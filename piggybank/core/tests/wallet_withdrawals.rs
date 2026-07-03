//! Integration tests for the user-wallet withdrawal saga — real Postgres **and**
//! TigerBeetle (no mocks, per the project rules). They run when `DATABASE_URL` is set
//! and a TigerBeetle replica is reachable (`nix run .#db` + `.#tb`), and skip
//! otherwise. Each test uses a fresh provisioned user, so runs are isolated on shared
//! infrastructure. The relay is driven explicitly via `Relay::drain` to apply
//! committed events deterministically; the custody broadcast is the [`StubCustody`]
//! no-op, so the saga's two-phase ledger behaviour (reserve → settle/void) is what's
//! under test.

use std::{
	collections::HashMap,
	sync::{Arc, Mutex},
};

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
		deposits::PgDeposits,
		dispatcher::Dispatcher,
		ledger::{self, TbLedger},
		relay::Relay,
		tigerbeetle::TigerBeetle,
		users::PgUsers,
		withdrawals::PgWithdrawals,
	},
	ports::{BroadcastRequest, Custody, CustodyError, DepositAddresses, UserRepository, WithdrawalRepository, ledger::Ledger},
};
use sqlx::PgPool;
use tokio::sync::Notify;
use uuid::Uuid;

// Address alphabets for the test-only deposit-address stub (defined at the bottom).
const BASE58: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
const BASE64URL: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

struct Harness {
	pool: PgPool,
	deposits: PgDeposits,
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
		deposits: PgDeposits::new(pool.clone()),
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
	balance_app::record_deposit(&h.deposits, &h.notify, unique_tx_ref(), Party::User(user), network, usdt(amount))
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
		&StubCustody,
		&h.notify,
		&Network::ALL,
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
		&StubCustody,
		&h.notify,
		&Network::ALL,
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
		&StubCustody,
		&h.notify,
		&Network::ALL,
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
		&StubCustody,
		&h.notify,
		&Network::ALL,
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

	h.users.disable(user).await.unwrap();

	let err = withdrawal_app::request_withdrawal(
		h.withdrawals.as_ref(),
		h.ledger.as_ref(),
		h.users.as_ref(),
		&StubCustody,
		&h.notify,
		&Network::ALL,
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
		&StubCustody,
		&h.notify,
		&Network::ALL,
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
	balance_app::seed_fund_capital(&h.deposits, &h.notify, Network::Ton, big).await.unwrap();
	h.relay.drain().await;
	let dispatched = withdrawal_app::dispatch_withdrawal(h.withdrawals.as_ref(), &StubCustody, &h.notify, withdrawal.id())
		.await
		.unwrap();
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
		&StubCustody,
		&h.notify,
		&Network::ALL,
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

/// The dispatch gate is `min(TB rail, on-chain treasury)`: a treasury holding zero USDT
/// on-chain queues the withdrawal even though the TB rail accounting looks liquid (it
/// counts confirmed deposits still on users' derived addresses, which the treasury hot
/// wallet cannot spend) — and the queued withdrawal stays user-cancellable (full refund).
#[tokio::test]
async fn an_onchain_short_treasury_queues_despite_a_liquid_tb_rail() {
	let Some(h) = harness().await else { return };
	let user = active_user(&h).await;
	let network = Network::Bep20;
	let claim = LedgerAccountKey::UserClaim(user);
	// The deposit credits the TB rail, so the accounting balance covers the net.
	deposit(&h, user, network, "100").await;

	let custody = TestCustody::with_view(network, TreasuryView::OnChain(Usdt::ZERO));
	let withdrawal = withdrawal_app::request_withdrawal(
		h.withdrawals.as_ref(),
		h.ledger.as_ref(),
		h.users.as_ref(),
		&custody,
		&h.notify,
		&Network::ALL,
		user,
		network,
		destination(network),
		usdt("50"),
	)
	.await
	.unwrap();
	assert_eq!(withdrawal.state(), WithdrawalState::Queued, "an on-chain-short treasury queues, never dispatches");
	h.relay.drain().await;
	assert_eq!(bal(&h, &claim).await.locked, usdt("50"), "the clearing reserve holds the gross while queued");

	// Queued = cancellable: the user gets the full gross back.
	let cancelled = withdrawal_app::cancel_withdrawal(h.withdrawals.as_ref(), &h.notify, withdrawal.id(), user).await.unwrap();
	assert_eq!(cancelled.state(), WithdrawalState::Cancelled);
	h.relay.drain().await;
	let refunded = bal(&h, &claim).await;
	assert_eq!(refunded.locked, Usdt::ZERO, "nothing remains reserved");
	assert_eq!(refunded.available(), usdt("100"), "the full balance is spendable again");
}

/// A treasury provably liquid on-chain (and a liquid TB rail) dispatches immediately.
#[tokio::test]
async fn an_onchain_liquid_treasury_dispatches_immediately() {
	let Some(h) = harness().await else { return };
	let user = active_user(&h).await;
	let network = Network::Bep20;
	deposit(&h, user, network, "100").await;

	let custody = TestCustody::with_view(network, TreasuryView::OnChain(usdt("1000000000")));
	let withdrawal = withdrawal_app::request_withdrawal(
		h.withdrawals.as_ref(),
		h.ledger.as_ref(),
		h.users.as_ref(),
		&custody,
		&h.notify,
		&Network::ALL,
		user,
		network,
		destination(network),
		usdt("50"),
	)
	.await
	.unwrap();
	assert_eq!(withdrawal.state(), WithdrawalState::Processing, "both liquidity sources cover the net — dispatched");
	h.relay.drain().await;

	// Settle so the shared rails aren't left with a dangling in-flight reservation.
	withdrawal_app::settle_withdrawal(h.withdrawals.as_ref(), &h.notify, withdrawal.id(), unique_tx_ref())
		.await
		.unwrap();
	h.relay.drain().await;
}

/// A treasury read failure DEGRADES to accept-and-queue — never a refusal: acceptance
/// and the clearing reserve must not depend on a flaky chain node.
#[tokio::test]
async fn a_treasury_read_failure_queues_and_never_rejects() {
	let Some(h) = harness().await else { return };
	let user = active_user(&h).await;
	let network = Network::Bep20;
	let claim = LedgerAccountKey::UserClaim(user);
	deposit(&h, user, network, "100").await;

	let custody = TestCustody::with_view(network, TreasuryView::Unreachable);
	let withdrawal = withdrawal_app::request_withdrawal(
		h.withdrawals.as_ref(),
		h.ledger.as_ref(),
		h.users.as_ref(),
		&custody,
		&h.notify,
		&Network::ALL,
		user,
		network,
		destination(network),
		usdt("50"),
	)
	.await
	.expect("a chain-view outage must not refuse the user");
	assert_eq!(withdrawal.state(), WithdrawalState::Queued, "degrade to queued on a treasury read failure");
	h.relay.drain().await;
	assert_eq!(bal(&h, &claim).await.locked, usdt("50"), "the reserve is untouched by the degraded gate");

	withdrawal_app::cancel_withdrawal(h.withdrawals.as_ref(), &h.notify, withdrawal.id(), user).await.unwrap();
	h.relay.drain().await;
}

/// The operator dispatch RPC refuses a provably on-chain-short rail — the withdrawal
/// stays queued (still cancellable) instead of marching into a custody park.
#[tokio::test]
async fn admin_dispatch_is_refused_when_the_treasury_is_short_onchain() {
	let Some(h) = harness().await else { return };
	let user = active_user(&h).await;
	let network = Network::Bep20;
	deposit(&h, user, network, "100").await;

	let custody = TestCustody::with_view(network, TreasuryView::OnChain(Usdt::ZERO));
	let withdrawal = withdrawal_app::request_withdrawal(
		h.withdrawals.as_ref(),
		h.ledger.as_ref(),
		h.users.as_ref(),
		&custody,
		&h.notify,
		&Network::ALL,
		user,
		network,
		destination(network),
		usdt("50"),
	)
	.await
	.unwrap();
	assert_eq!(withdrawal.state(), WithdrawalState::Queued);
	h.relay.drain().await;

	let err = withdrawal_app::dispatch_withdrawal(h.withdrawals.as_ref(), &custody, &h.notify, withdrawal.id())
		.await
		.unwrap_err();
	assert!(matches!(err, DomainError::Validation(_)), "an underfunded rail refuses the dispatch, got {err:?}");
	let after = h.withdrawals.find_by_id(withdrawal.id()).await.unwrap().unwrap();
	assert_eq!(after.state(), WithdrawalState::Queued, "a refused dispatch leaves the withdrawal queued");

	withdrawal_app::cancel_withdrawal(h.withdrawals.as_ref(), &h.notify, withdrawal.id(), user).await.unwrap();
	h.relay.drain().await;
}

/// The dispatcher worker drains the accept-and-queue backlog: a queued withdrawal is
/// dispatched exactly once when BOTH gates pass (TB rail + on-chain treasury), stays
/// queued while the treasury is short on-chain, and a second sweep no-ops (the state is
/// no longer `queued`; dispatch itself is idempotent).
#[tokio::test]
async fn the_dispatcher_sweeps_a_queued_withdrawal_once_both_gates_pass() {
	let Some(h) = harness().await else { return };
	let user = active_user(&h).await;
	// TON keeps this test's dispatchable row off the BEP20 rail other tests queue on; the
	// baseline all-rails-short custody view keeps THEIR queued rows out of this sweep.
	let network = Network::Ton;
	deposit(&h, user, Network::Bep20, "100").await;
	// A small top-up so the TB TON gate covers the net without dwarfing the shared rail.
	balance_app::seed_fund_capital(&h.deposits, &h.notify, network, usdt("60")).await.unwrap();
	h.relay.drain().await;

	let custody = Arc::new(TestCustody::short_everywhere());
	let withdrawal = withdrawal_app::request_withdrawal(
		h.withdrawals.as_ref(),
		h.ledger.as_ref(),
		h.users.as_ref(),
		custody.as_ref(),
		&h.notify,
		&Network::ALL,
		user,
		network,
		destination(network),
		usdt("50"),
	)
	.await
	.unwrap();
	assert_eq!(withdrawal.state(), WithdrawalState::Queued, "the on-chain-short treasury queues the request");
	h.relay.drain().await;

	let dispatcher = Dispatcher::new(h.pool.clone(), h.withdrawals.clone(), h.ledger.clone(), custody.clone(), h.notify.clone());

	// On-chain still short — the sweep leaves everything queued.
	assert_eq!(dispatcher.sweep().await.unwrap(), 0, "an on-chain-short rail dispatches nothing");
	let still_queued = h.withdrawals.find_by_id(withdrawal.id()).await.unwrap().unwrap();
	assert_eq!(still_queued.state(), WithdrawalState::Queued, "still queued (and cancellable) while the treasury is short");

	// The treasury is topped up on-chain — the next sweep dispatches it.
	custody.set(network, TreasuryView::OnChain(usdt("1000000000")));
	assert!(dispatcher.sweep().await.unwrap() >= 1, "a topped-up rail dispatches the queued withdrawal");
	let processing = h.withdrawals.find_by_id(withdrawal.id()).await.unwrap().unwrap();
	assert_eq!(processing.state(), WithdrawalState::Processing, "the dispatcher moved it to processing");
	h.relay.drain().await;

	// A second sweep no-ops for this withdrawal — it is no longer queued.
	dispatcher.sweep().await.unwrap();
	let after = h.withdrawals.find_by_id(withdrawal.id()).await.unwrap().unwrap();
	assert_eq!(after.state(), WithdrawalState::Processing, "a second sweep does not re-dispatch");

	// Settle so the shared rails aren't left with a dangling in-flight reservation.
	withdrawal_app::settle_withdrawal(h.withdrawals.as_ref(), &h.notify, withdrawal.id(), unique_tx_ref())
		.await
		.unwrap();
	h.relay.drain().await;
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

// ── Test-only custody with a configurable on-chain treasury view ────────────────
// A `Custody` port adapter over no chain (Postgres/TigerBeetle stay real, per the
// no-DB-mocks rule) — like `StubCustody`, but its `treasury_liquidity` serves a
// per-rail view the test controls, so the dispatch gate's three arms (`Some`, `None`,
// `Err`) are each drivable. Broadcasts are stub no-ops.

/// One rail's on-chain treasury as the test custody reports it. A rail with no entry
/// has no chain view at all (`Ok(None)`), like an unwired rail.
#[derive(Clone, Copy)]
enum TreasuryView {
	/// The treasury holds this much spendable USDT on-chain.
	OnChain(Usdt),
	/// The chain read fails (node outage).
	Unreachable,
}

struct TestCustody {
	views: Mutex<HashMap<Network, TreasuryView>>,
}

impl TestCustody {
	fn with_view(network: Network, view: TreasuryView) -> Self {
		Self {
			views: Mutex::new(HashMap::from([(network, view)])),
		}
	}

	/// Every rail short on-chain (`OnChain(0)`) — the dispatcher test's baseline, so a
	/// concurrently-queued withdrawal from a parallel test can never be swept up by it.
	fn short_everywhere() -> Self {
		Self {
			views: Mutex::new(Network::ALL.iter().map(|network| (*network, TreasuryView::OnChain(Usdt::ZERO))).collect()),
		}
	}

	fn set(&self, network: Network, view: TreasuryView) {
		self.views.lock().unwrap().insert(network, view);
	}
}

impl domain::architecture::Gateway for TestCustody {}

#[async_trait]
impl Custody for TestCustody {
	async fn broadcast(&self, request: &BroadcastRequest) -> Result<(), CustodyError> {
		let _ = request;
		Ok(())
	}

	async fn treasury_liquidity(&self, network: Network) -> Result<Option<Usdt>, CustodyError> {
		match self.views.lock().unwrap().get(&network) {
			Some(TreasuryView::OnChain(balance)) => Ok(Some(*balance)),
			Some(TreasuryView::Unreachable) => Err(CustodyError::Unavailable("test: chain view down".into())),
			None => Ok(None),
		}
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

impl domain::architecture::Gateway for StubDepositAddresses {}

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
