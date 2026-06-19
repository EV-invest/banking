//! Integration tests for the user-wallet withdrawal saga — real Postgres **and**
//! TigerBeetle (no mocks, per the project rules). They run when `DATABASE_URL` is set
//! and a TigerBeetle replica is reachable (`nix run .#db` + `.#tb`), and skip
//! otherwise. Each test uses a fresh provisioned user, so runs are isolated on shared
//! infrastructure. The relay is driven explicitly via `Relay::drain` to apply
//! committed events deterministically; the custody broadcast is the [`StubCustody`]
//! no-op, so the saga's two-phase ledger behaviour (reserve → settle/void) is what's
//! under test.

use std::sync::Arc;

use domain::{
	auth::AuthSubject,
	balance::{LedgerAccountKey, Party},
	error::DomainError,
	money::{Network, TxRef, Usdt, WalletAddress},
	users::{Email, UserId},
};
use piggybank_core::{
	application::{balance as balance_app, withdrawals as withdrawal_app},
	infrastructure::{
		custody::StubCustody,
		db,
		deposit_addresses::StubDepositAddresses,
		ledger::{self, TbLedger},
		relay::Relay,
		tigerbeetle::TigerBeetle,
		users::PgUsers,
		withdrawals::PgWithdrawals,
	},
	ports::{DepositAddresses, LedgerBalance, UserRepository, WithdrawalRepository, ledger::Ledger},
};
use sqlx::PgPool;
use tokio::sync::Notify;
use uuid::Uuid;

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

async fn bal(h: &Harness, key: &LedgerAccountKey) -> LedgerBalance {
	h.ledger.balance(key).await.unwrap()
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
	let claim = LedgerAccountKey::UserClaim(user, network);
	let fee_account = LedgerAccountKey::FeeRevenue(network);

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
	assert_eq!(bal(&h, &fee_account).await.posted.checked_sub(fee_before).unwrap(), usdt("1"), "the fee was retained");
}

#[tokio::test]
async fn withdraw_fail_voids_and_refunds_in_full() {
	let Some(h) = harness().await else { return };
	let user = active_user(&h).await;
	let network = Network::Trc20;
	let claim = LedgerAccountKey::UserClaim(user, network);

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
async fn deposit_address_is_stable_per_user_and_network() {
	let Some(h) = harness().await else { return };
	let user = active_user(&h).await;
	for network in Network::ALL {
		let first = h.deposit_addresses.address(user, network).await.unwrap();
		let second = h.deposit_addresses.address(user, network).await.unwrap();
		assert_eq!(first, second, "the cached deposit address is stable across reads");
		assert_eq!(first.network(), network, "the address is for the requested network");
	}
}
