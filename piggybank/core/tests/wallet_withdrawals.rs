//! Integration tests for the user-wallet withdrawal saga — real Postgres **and**
//! TigerBeetle (no mocks, per the project rules). They run when `DATABASE_URL` is set
//! and a TigerBeetle replica is reachable (`nix run .#db` + `.#tb`), and skip
//! otherwise. Each test uses a fresh provisioned user, so runs are isolated on shared
//! infrastructure. The relay is driven explicitly via `Relay::drain` to apply
//! committed events deterministically; the custody broadcast is the [`StubCustody`]
//! no-op, so the saga's two-phase ledger behaviour (reserve → settle/void) is what's
//! under test.

use std::sync::Arc;

use async_trait::async_trait;
use domain::{
	auth::AuthSubject,
	balance::{LedgerAccountKey, Party},
	error::DomainError,
	money::{Network, TxRef, Usdt, WalletAddress},
	users::{Email, UserId},
	withdrawals::WithdrawalState,
};
use piggybank_core::{
	application::{balance as balance_app, withdrawals as withdrawal_app},
	infrastructure::{
		custody::StubCustody,
		db,
		ledger::{self, TbLedger},
		relay::Relay,
		tigerbeetle::TigerBeetle,
		users::PgUsers,
		withdrawals::PgWithdrawals,
	},
	ports::{DepositAddresses, UserRepository, WithdrawalRepository, ledger::Ledger},
};
use sqlx::PgPool;
use tokio::sync::Notify;
use uuid::Uuid;

// Address alphabets for the test-only deposit-address stub (defined at the bottom).
const BASE58: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
const BASE64URL: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

struct Harness {
	pool: PgPool,
	ledger: Arc<dyn Ledger>,
	withdrawals: Arc<dyn WithdrawalRepository>,
	users: Arc<dyn UserRepository>,
	deposit_addresses: Arc<dyn DepositAddresses>,
	relay: Relay,
	notify: Arc<Notify>,
}

async fn harness() -> Option<Harness> {
	let url = std::env::var("DATABASE_URL").ok().filter(|s| !s.is_empty())?;
	let pool = db::connect(&url).await.expect("connect to Postgres");
	db::migrate(&pool).await.expect("apply migrations");

	let address = std::env::var("TIGERBEETLE_ADDRESS").unwrap_or_else(|_| "127.0.0.1:3033".to_owned());
	let cluster = std::env::var("TIGERBEETLE_CLUSTER_ID").ok().and_then(|s| s.parse().ok()).unwrap_or(0u128);
	let tigerbeetle = Arc::new(TigerBeetle::connect(cluster, &address).expect("connect to TigerBeetle"));
	let ledger: Arc<dyn Ledger> = Arc::new(TbLedger::new(tigerbeetle, pool.clone()));
	if ledger::seed_singletons(ledger.as_ref()).await.is_err() {
		eprintln!("TigerBeetle unreachable — skipping withdrawal test");
		return None;
	}

	let withdrawals: Arc<dyn WithdrawalRepository> = Arc::new(PgWithdrawals::new(pool.clone()));
	let users: Arc<dyn UserRepository> = Arc::new(PgUsers::new(pool.clone()));
	let deposit_addresses: Arc<dyn DepositAddresses> = Arc::new(StubDepositAddresses::new(pool.clone()));
	let notify = Arc::new(Notify::new());
	let relay = Relay::new(pool.clone(), ledger.clone(), Arc::new(StubCustody), notify.clone());
	Some(Harness {
		pool,
		ledger,
		withdrawals,
		users,
		deposit_addresses,
		relay,
		notify,
	})
}

fn usdt(decimal: &str) -> Usdt {
	Usdt::parse_decimal(decimal).unwrap()
}

fn unique_tx_ref() -> TxRef {
	TxRef::parse(&format!("itest-{}", Uuid::new_v4())).unwrap()
}

/// A destination address valid for `network` (distinct from the user's own).
fn destination(network: Network) -> WalletAddress {
	let raw = match network {
		Network::Bep20 => "0x52908400098527886E0F7030069857D2E4169EE7",
		Network::Trc20 => "TJRabPrwbZy45sbavfcjinPJC18kjpRTv8",
		Network::Ton => "EQCD39VS5jcptHL8vMjEXrzGaRcCVYto7HUn4bpAOg8xqB2N",
	};
	WalletAddress::parse(network, raw).unwrap()
}

async fn active_user(h: &Harness) -> UserId {
	let subject = AuthSubject::parse(&format!("itest-{}", Uuid::new_v4())).unwrap();
	let email = Email::parse(&format!("u{}@example.com", Uuid::new_v4().simple())).unwrap();
	h.users.provision(subject, email, true).await.unwrap().id()
}

/// A `Usdt`-typed view of a ledger balance for assertions (the ledger port speaks raw
/// base units; the cash ledger's unit is 18-dp USDT).
struct Bal {
	posted: Usdt,
	#[allow(dead_code)]
	pending: Usdt,
	locked: Usdt,
}

impl Bal {
	fn available(&self) -> Usdt {
		self.posted.checked_sub(self.locked).unwrap_or(Usdt::ZERO)
	}
}

async fn bal(h: &Harness, key: &LedgerAccountKey) -> Bal {
	let b = h.ledger.balance(key).await.unwrap();
	Bal {
		posted: Usdt::from_base_units(b.posted),
		pending: Usdt::from_base_units(b.pending),
		locked: Usdt::from_base_units(b.locked),
	}
}

async fn deposit(h: &Harness, user: UserId, network: Network, amount: &str) {
	balance_app::record_deposit(&h.pool, &h.notify, unique_tx_ref(), Party::User(user), network, usdt(amount))
		.await
		.unwrap();
	h.relay.drain().await;
}

#[tokio::test]
async fn withdraw_reserves_then_settles_and_retains_fee() {
	let Some(h) = harness().await else { return };
	let user = active_user(&h).await;
	let network = Network::Bep20;
	let claim = LedgerAccountKey::UserClaim(user);
	let fee_account = LedgerAccountKey::FeeRevenue;

	deposit(&h, user, network, "100").await;
	let fee_before = bal(&h, &fee_account).await.posted;

	// Request a 50 USDT withdrawal (fee 1, net 49) — the gross is reserved as pending.
	let withdrawal = withdrawal_app::request_withdrawal(
		h.withdrawals.as_ref(),
		h.ledger.as_ref(),
		h.users.as_ref(),
		&h.notify,
		user,
		network,
		destination(network),
		usdt("50"),
	)
	.await
	.unwrap();
	assert_eq!(withdrawal.net_amount(), usdt("49"));
	h.relay.drain().await;

	let reserved = bal(&h, &claim).await;
	assert_eq!(reserved.posted, usdt("100"), "settled balance unchanged until the withdrawal settles");
	assert_eq!(reserved.locked, usdt("50"), "the gross is locked as a pending debit");
	assert_eq!(reserved.available(), usdt("50"), "available drops by the reserved gross");

	// Settle on confirmations — posts both legs.
	withdrawal_app::settle_withdrawal(h.withdrawals.as_ref(), &h.notify, withdrawal.id(), unique_tx_ref())
		.await
		.unwrap();
	h.relay.drain().await;

	let settled = bal(&h, &claim).await;
	assert_eq!(settled.posted, usdt("50"), "the gross left the user's claim");
	assert_eq!(settled.locked, Usdt::ZERO, "nothing remains reserved");
	// FeeRevenue is now a network-agnostic singleton shared across (parallel) tests, so
	// assert this withdrawal credited *at least* its fee — concurrent tests only add more.
	assert!(bal(&h, &fee_account).await.posted.checked_sub(fee_before).unwrap() >= usdt("1"), "the fee was retained");
}

#[tokio::test]
async fn withdraw_fail_voids_and_refunds_in_full() {
	let Some(h) = harness().await else { return };
	let user = active_user(&h).await;
	let network = Network::Trc20;
	let claim = LedgerAccountKey::UserClaim(user);

	deposit(&h, user, network, "100").await;

	let withdrawal = withdrawal_app::request_withdrawal(
		h.withdrawals.as_ref(),
		h.ledger.as_ref(),
		h.users.as_ref(),
		&h.notify,
		user,
		network,
		destination(network),
		usdt("30"),
	)
	.await
	.unwrap();
	h.relay.drain().await;
	assert_eq!(bal(&h, &claim).await.locked, usdt("30"), "the gross is reserved");

	// The broadcast never landed — fail it. Both legs void; the user is made whole.
	withdrawal_app::fail_withdrawal(h.withdrawals.as_ref(), &h.notify, withdrawal.id()).await.unwrap();
	h.relay.drain().await;

	let refunded = bal(&h, &claim).await;
	assert_eq!(refunded.posted, usdt("100"), "the reservation was voided");
	assert_eq!(refunded.locked, Usdt::ZERO, "nothing remains reserved");
	assert_eq!(refunded.available(), usdt("100"), "the full balance is spendable again");
}

#[tokio::test]
async fn withdraw_below_minimum_is_rejected() {
	let Some(h) = harness().await else { return };
	let user = active_user(&h).await;
	let network = Network::Ton;
	let err = withdrawal_app::request_withdrawal(
		h.withdrawals.as_ref(),
		h.ledger.as_ref(),
		h.users.as_ref(),
		&h.notify,
		user,
		network,
		destination(network),
		usdt("5"),
	)
	.await
	.unwrap_err();
	assert!(matches!(err, DomainError::Validation(_)), "below-minimum is a validation error, got {err:?}");
}

#[tokio::test]
async fn withdraw_beyond_available_is_rejected_read_first() {
	let Some(h) = harness().await else { return };
	let user = active_user(&h).await;
	let network = Network::Bep20;
	deposit(&h, user, network, "10").await;

	// 50 clears the minimum but exceeds the available balance — Read-First rejects it.
	let err = withdrawal_app::request_withdrawal(
		h.withdrawals.as_ref(),
		h.ledger.as_ref(),
		h.users.as_ref(),
		&h.notify,
		user,
		network,
		destination(network),
		usdt("50"),
	)
	.await
	.unwrap_err();
	assert!(matches!(err, DomainError::Validation(_)), "insufficient available is a validation error, got {err:?}");
}

#[tokio::test]
async fn a_disabled_user_cannot_withdraw() {
	let Some(h) = harness().await else { return };
	let user = active_user(&h).await;
	let network = Network::Trc20;
	deposit(&h, user, network, "100").await;

	let mut account = h.users.find_by_id(user).await.unwrap().unwrap();
	account.disable();
	h.users.save(&mut account).await.unwrap();

	let err = withdrawal_app::request_withdrawal(
		h.withdrawals.as_ref(),
		h.ledger.as_ref(),
		h.users.as_ref(),
		&h.notify,
		user,
		network,
		destination(network),
		usdt("50"),
	)
	.await
	.unwrap_err();
	assert!(matches!(err, DomainError::Forbidden(_)), "a disabled account is forbidden from withdrawing, got {err:?}");
}

#[tokio::test]
async fn withdraw_on_a_short_rail_is_queued_then_dispatched() {
	let Some(h) = harness().await else { return };
	let user = active_user(&h).await;
	let claim = LedgerAccountKey::UserClaim(user);
	// 1e9 USDT dwarfs any liquidity the shared TON rail accumulates across tests, so the
	// short-rail (queue) path is deterministic regardless of test interleaving.
	let big = usdt("1000000000");

	// Fund the user's unified claim via BEP20; the TON rail cannot possibly cover 1e9.
	deposit(&h, user, Network::Bep20, "1000000000").await;

	// Withdraw on TON: the user is solvent but the TON rail is short, so the withdrawal
	// is accepted and QUEUED (accept-and-queue), not refused.
	let withdrawal = withdrawal_app::request_withdrawal(
		h.withdrawals.as_ref(),
		h.ledger.as_ref(),
		h.users.as_ref(),
		&h.notify,
		user,
		Network::Ton,
		destination(Network::Ton),
		big,
	)
	.await
	.unwrap();
	assert_eq!(withdrawal.state(), WithdrawalState::Queued, "a short rail queues, it does not refuse");
	h.relay.drain().await;
	assert_eq!(bal(&h, &claim).await.locked, big, "the gross is reserved while queued");

	// The treasury tops up the TON rail past the net; the worker then dispatches it.
	balance_app::seed_fund_capital(&h.pool, &h.notify, Network::Ton, big).await.unwrap();
	h.relay.drain().await;
	let dispatched = withdrawal_app::dispatch_withdrawal(h.withdrawals.as_ref(), &h.notify, withdrawal.id()).await.unwrap();
	assert_eq!(dispatched.state(), WithdrawalState::Processing, "a funded rail dispatches");
	h.relay.drain().await;

	// Settle it — the gross leaves the claim (fresh user, so deterministic).
	withdrawal_app::settle_withdrawal(h.withdrawals.as_ref(), &h.notify, withdrawal.id(), unique_tx_ref())
		.await
		.unwrap();
	h.relay.drain().await;
	let settled = bal(&h, &claim).await;
	assert_eq!(settled.posted, Usdt::ZERO, "the gross left the claim after settle");
	assert_eq!(settled.locked, Usdt::ZERO, "nothing remains reserved");
}

#[tokio::test]
async fn a_queued_withdrawal_can_be_cancelled_and_refunds() {
	let Some(h) = harness().await else { return };
	let user = active_user(&h).await;
	let claim = LedgerAccountKey::UserClaim(user);
	let big = usdt("1000000000");
	deposit(&h, user, Network::Bep20, "1000000000").await;

	// Withdraw on the TRC20 rail, which cannot cover 1e9 → queued (no test seeds TRC20).
	let withdrawal = withdrawal_app::request_withdrawal(
		h.withdrawals.as_ref(),
		h.ledger.as_ref(),
		h.users.as_ref(),
		&h.notify,
		user,
		Network::Trc20,
		destination(Network::Trc20),
		big,
	)
	.await
	.unwrap();
	assert_eq!(withdrawal.state(), WithdrawalState::Queued);
	h.relay.drain().await;
	assert_eq!(bal(&h, &claim).await.locked, big);

	// The user cancels the queued withdrawal — full refund, nothing was broadcast.
	let cancelled = withdrawal_app::cancel_withdrawal(h.withdrawals.as_ref(), &h.notify, withdrawal.id(), user).await.unwrap();
	assert_eq!(cancelled.state(), WithdrawalState::Cancelled);
	h.relay.drain().await;
	let refunded = bal(&h, &claim).await;
	assert_eq!(refunded.locked, Usdt::ZERO, "nothing remains reserved");
	assert_eq!(refunded.available(), big, "the full balance is spendable again");
}

#[tokio::test]
async fn deposit_address_is_stable_per_user_and_network() {
	let Some(h) = harness().await else { return };
	let user = active_user(&h).await;
	for network in Network::ALL {
		let first = h.deposit_addresses.address(user, network).await.unwrap().expect("stub yields a fundable address");
		let second = h.deposit_addresses.address(user, network).await.unwrap().expect("stub yields a fundable address");
		assert_eq!(first, second, "the cached deposit address is stable across reads");
		assert_eq!(first.network(), network, "the address is for the requested network");
	}
}

// ── Test-only deposit-address stub ──────────────────────────────────────────────
// Stands in for HD derivation from the fund's xpub so the saga runs without the
// separate signer process. It deterministically derives a *structurally valid* (not
// spendable) per-(user, network) address and caches it. Production wires the
// signer-backed `SignerDepositAddresses`; this double lives only here.

struct StubDepositAddresses {
	pool: PgPool,
}

impl StubDepositAddresses {
	fn new(pool: PgPool) -> Self {
		Self { pool }
	}
}

#[async_trait]
impl DepositAddresses for StubDepositAddresses {
	async fn address(&self, user: UserId, network: Network) -> Result<Option<WalletAddress>, DomainError> {
		// The stub stands in for a working derivation, so it caches a `derived` (fundable)
		// address — the placeholder-gating path is exercised by the signer-adapter tests.
		if let Some(existing) = sqlx::query_scalar::<_, String>("SELECT address FROM user_deposit_addresses WHERE user_id = $1 AND network = $2")
			.bind(user.raw())
			.bind(network.as_str())
			.fetch_optional(&self.pool)
			.await
			.map_err(repo_err)?
		{
			return Ok(Some(WalletAddress::parse(network, &existing)?));
		}
		let derived = derive_address(user, network);
		sqlx::query("INSERT INTO user_deposit_addresses (user_id, network, address, address_kind) VALUES ($1, $2, $3, 'derived') ON CONFLICT (user_id, network) DO NOTHING")
			.bind(user.raw())
			.bind(network.as_str())
			.bind(derived.as_str())
			.execute(&self.pool)
			.await
			.map_err(repo_err)?;
		Ok(Some(derived))
	}
}

/// Deterministic per-(user, network) material via chained UUID v5 (stable across
/// runs/platforms — SHA-1 with fixed namespaces, no RNG).
fn derive_bytes(seed: &str, n: usize) -> Vec<u8> {
	let mut out = Vec::with_capacity(n);
	let mut acc = Uuid::new_v5(&Uuid::NAMESPACE_OID, seed.as_bytes());
	while out.len() < n {
		out.extend_from_slice(acc.as_bytes());
		acc = Uuid::new_v5(&acc, seed.as_bytes());
	}
	out.truncate(n);
	out
}

/// A structurally valid address for `network` from the derived bytes — each byte mapped
/// onto the chain's alphabet so [`WalletAddress::parse`] always accepts it.
fn derive_address(user: UserId, network: Network) -> WalletAddress {
	let seed = format!("{user}:{network}");
	let address = match network {
		Network::Bep20 => {
			let bytes = derive_bytes(&seed, 20);
			let mut s = String::from("0x");
			for byte in bytes {
				s.push_str(&format!("{byte:02x}"));
			}
			s
		}
		Network::Trc20 => {
			let bytes = derive_bytes(&seed, 33);
			let mut s = String::from("T");
			for byte in bytes {
				s.push(BASE58[byte as usize % BASE58.len()] as char);
			}
			s
		}
		Network::Ton => {
			let bytes = derive_bytes(&seed, 48);
			let mut s = String::with_capacity(48);
			for byte in bytes {
				s.push(BASE64URL[byte as usize % BASE64URL.len()] as char);
			}
			s
		}
	};
	WalletAddress::parse(network, &address).expect("derived stub address is structurally valid by construction")
}

fn repo_err(err: sqlx::Error) -> DomainError {
	DomainError::Repository(err.to_string())
}

#[test]
fn derived_addresses_are_valid_and_stable() {
	let user = UserId::new();
	for network in Network::ALL {
		let a = derive_address(user, network);
		let b = derive_address(user, network);
		assert_eq!(a, b, "derivation must be deterministic");
		assert_eq!(a.network(), network);
		assert!(WalletAddress::parse(network, a.as_str()).is_ok());
	}
}
