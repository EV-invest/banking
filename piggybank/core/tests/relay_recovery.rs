//! Integration tests for FB-05 — parked outbox events + reconciliation + the saga reaper.
//! Real Postgres **and** TigerBeetle (no mocks, per the project rules). They run when
//! `DATABASE_URL` is set and a TigerBeetle replica is reachable (`nix run .#db` + `.#tb`),
//! and skip otherwise. The relay is driven explicitly via `Relay::drain`; the
//! reconciliation/reaper jobs via their public `scan`/`sweep` (no waiting on the interval).
//!
//! What's proven:
//!   - a non-retryable event is moved to the distinct **parked** terminal state — NOT
//!     marked dispatched — so it stays queryable and is surfaced by reconciliation
//!     (BANK-FAULT-01 / BANK-ARCH-05);
//!   - an abandoned `processing` withdrawal is surfaced by the reaper (alert-only, never
//!     auto-voided), and an abandoned `queued` withdrawal is auto-cancelled (BANK-FAULT-04).

use std::{
	sync::{
		Arc,
		atomic::{AtomicUsize, Ordering},
	},
	time::Duration,
};

use async_trait::async_trait;
use domain::{
	architecture::Gateway,
	auth::AuthSubject,
	balance::Party,
	money::{Network, TxRef, Usdt, WalletAddress},
	users::{Email, UserId},
	withdrawals::{WithdrawalEvent, WithdrawalState},
};
use piggybank_core::{
	application::{balance as balance_app, withdrawals as withdrawal_app},
	infrastructure::{
		custody::StubCustody,
		db,
		deposits::PgDeposits,
		ledger::{self, TbLedger},
		reaper::Reaper,
		reconciliation::Reconciliation,
		redemptions::PgRedemptions,
		relay::Relay,
		tigerbeetle::TigerBeetle,
		users::PgUsers,
		withdrawals::PgWithdrawals,
	},
	ports::{BroadcastRequest, Custody, CustodyError, RedemptionRepository, UserRepository, WithdrawalRepository, ledger::Ledger},
};
use sqlx::PgPool;
use tokio::sync::Notify;
use uuid::Uuid;

struct Harness {
	pool: PgPool,
	deposits: PgDeposits,
	ledger: Arc<dyn Ledger>,
	withdrawals: Arc<dyn WithdrawalRepository>,
	redemptions: Arc<dyn RedemptionRepository>,
	users: Arc<dyn UserRepository>,
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
		eprintln!("TigerBeetle unreachable — skipping relay recovery test");
		return None;
	}

	let withdrawals: Arc<dyn WithdrawalRepository> = Arc::new(PgWithdrawals::new(pool.clone()));
	let redemptions: Arc<dyn RedemptionRepository> = Arc::new(PgRedemptions::new(pool.clone()));
	let users: Arc<dyn UserRepository> = Arc::new(PgUsers::new(pool.clone()));
	let notify = Arc::new(Notify::new());
	let relay = Relay::new(pool.clone(), ledger.clone(), Arc::new(StubCustody), notify.clone());
	Some(Harness {
		deposits: PgDeposits::new(pool.clone()),
		pool,
		ledger,
		withdrawals,
		redemptions,
		users,
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

/// A non-retryable event must be moved to the distinct **parked** state, never marked
/// dispatched. We inject an unplannable outbox row (a `withdrawals` event whose payload is
/// not a `WithdrawalEvent`), which `plan()` rejects → the relay parks it. We then assert:
/// it stays undispatched-but-parked (so the drain skips it without dropping it), it stays
/// queryable, and reconciliation surfaces it in the parked-row scan.
#[tokio::test]
async fn a_parked_event_is_not_dispatched_and_reconciliation_surfaces_it() {
	let Some(h) = harness().await else { return };
	let event_id = Uuid::new_v4();
	let aggregate_id = Uuid::new_v4();
	sqlx::query("INSERT INTO outbox (event_id, aggregate, aggregate_id, kind, payload) VALUES ($1, 'withdrawals', $2, 'withdrawals', $3::jsonb)")
		.bind(event_id)
		.bind(aggregate_id)
		.bind("\"not-a-withdrawal-event\"")
		.execute(&h.pool)
		.await
		.expect("inject the unplannable outbox row");

	h.relay.drain().await;

	// Parked, NOT dispatched — the row stays queryable with its last_error, and is excluded
	// from the drain (parked_at set, dispatched_at null). We read the two timestamps as
	// booleans so the test needs no `time`/`chrono` decode of a column it never inspects.
	let (is_dispatched, is_parked, last_error): (bool, bool, Option<String>) =
		sqlx::query_as("SELECT dispatched_at IS NOT NULL, parked_at IS NOT NULL, last_error FROM outbox WHERE event_id = $1")
			.bind(event_id)
			.fetch_one(&h.pool)
			.await
			.expect("the parked row is still queryable");
	assert!(!is_dispatched, "a parked event must NOT be marked dispatched");
	assert!(is_parked, "a parked event must be in the distinct parked terminal state");
	assert!(last_error.is_some_and(|e| e.contains("unplannable")), "the park reason is recorded for forensics");

	// Reconciliation's parked-row scan surfaces it.
	let report = Reconciliation::new(h.pool.clone(), h.ledger.clone()).scan().await.expect("reconciliation scan");
	assert!(report.parked_rows >= 1, "reconciliation must surface the parked row");
	assert!(report.uncompensated_parked >= 1, "an un-compensated park is reported for intervention");
}

/// The reaper owns the timeout for abandoned sagas. A `processing` withdrawal past the max
/// age is surfaced (alert-only — never auto-voided, per the cardinal rule), while a
/// `queued` withdrawal past the max age is auto-cancelled (safe — never broadcast).
#[tokio::test]
async fn the_reaper_alerts_on_stuck_processing_and_reaps_queued_withdrawals() {
	let Some(h) = harness().await else { return };
	let network = Network::Bep20;

	// A `processing` withdrawal: deposit, request with a liquid rail (auto-dispatched →
	// processing), then backdate its last transition past the reaper's window.
	let processing_user = active_user(&h).await;
	balance_app::record_deposit(&h.deposits, &h.notify, unique_tx_ref(), Party::User(processing_user), network, usdt("100"))
		.await
		.unwrap();
	h.relay.drain().await;
	// Seed the rail so the request auto-dispatches to `processing`.
	balance_app::seed_fund_capital(&h.deposits, &h.notify, network, usdt("100")).await.unwrap();
	h.relay.drain().await;
	let processing = withdrawal_app::request_withdrawal(
		h.withdrawals.as_ref(),
		h.ledger.as_ref(),
		h.users.as_ref(),
		&StubCustody,
		&h.notify,
		&Network::ALL,
		processing_user,
		network,
		destination(network),
		usdt("50"),
	)
	.await
	.unwrap();
	assert_eq!(processing.state(), WithdrawalState::Processing, "a liquid rail auto-dispatches to processing");
	h.relay.drain().await;
	backdate_withdrawal(&h.pool, processing.id().raw()).await;

	// A `queued` withdrawal: the unified claim is funded by a large deposit on a DIFFERENT
	// rail (Bep20), and the withdrawal targets TRC20 for a gross so large no parallel test's
	// rail liquidity can cover it — so it is accepted-and-queued, not dispatched (rails are
	// global singletons shared across the test binaries, so this must not rely on a short
	// rail by default). Then backdate it past the reaper window.
	let queued_user = active_user(&h).await;
	let short_network = Network::Trc20;
	let big = usdt("1000000000");
	balance_app::record_deposit(&h.deposits, &h.notify, unique_tx_ref(), Party::User(queued_user), network, big)
		.await
		.unwrap();
	h.relay.drain().await;
	let queued = withdrawal_app::request_withdrawal(
		h.withdrawals.as_ref(),
		h.ledger.as_ref(),
		h.users.as_ref(),
		&StubCustody,
		&h.notify,
		&Network::ALL,
		queued_user,
		short_network,
		destination(short_network),
		big,
	)
	.await
	.unwrap();
	assert_eq!(queued.state(), WithdrawalState::Queued, "a gross beyond any rail's liquidity leaves the withdrawal queued");
	h.relay.drain().await;
	backdate_withdrawal(&h.pool, queued.id().raw()).await;

	// Sweep with a 30-minute window: our rows are backdated an hour (abandoned), while any
	// queued saga a parallel test binary left behind is seconds old and stays untouched — so
	// the shared rails/DB don't make this flaky.
	let reaper = Reaper::new(h.pool.clone(), h.withdrawals.clone(), h.redemptions.clone(), h.notify.clone()).with_max_age(Duration::from_secs(30 * 60));
	let report = reaper.sweep().await.expect("reaper sweep");

	assert!(report.stuck_processing_withdrawals >= 1, "the stuck processing withdrawal is surfaced (alert-only)");
	assert!(report.reaped_queued_withdrawals >= 1, "the abandoned queued withdrawal is auto-cancelled");

	// The processing one is untouched (alert-only); the queued one is now cancelled.
	let after_processing = h.withdrawals.find_by_id(processing.id()).await.unwrap().unwrap();
	assert_eq!(after_processing.state(), WithdrawalState::Processing, "the reaper never auto-voids a processing withdrawal");
	let after_queued = h.withdrawals.find_by_id(queued.id()).await.unwrap().unwrap();
	assert_eq!(after_queued.state(), WithdrawalState::Cancelled, "the abandoned queued withdrawal was refunded");
}

/// The broadcast-state guard: a `Dispatched` event unparked AFTER the withdrawal was
/// failed (its clearing reservation voided, the user refunded) must be parked again —
/// custody is never called, so the unpark-after-fail double-pay hazard (a real on-chain
/// send with nothing locked behind it) is structurally impossible, not just a runbook
/// discipline. The parked `Dispatched` row is injected to mirror the incident shape (a
/// custody park), because a live one is only drainable while the row is `processing`.
#[tokio::test]
async fn an_unparked_dispatch_after_fail_is_reparked_and_never_broadcast() {
	let Some(h) = harness().await else { return };
	let network = Network::Trc20;
	let user = active_user(&h).await;
	// Fund the claim on BEP20 and withdraw a gross no rail can cover, so the request is
	// accepted-and-queued deterministically on the shared rails (same shape as the reaper
	// test); the reserve then applies and its saga step is recorded.
	let big = usdt("1000000000");
	balance_app::record_deposit(&h.deposits, &h.notify, unique_tx_ref(), Party::User(user), Network::Bep20, big)
		.await
		.unwrap();
	h.relay.drain().await;
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
		big,
	)
	.await
	.unwrap();
	assert_eq!(withdrawal.state(), WithdrawalState::Queued);
	h.relay.drain().await;

	// Operator dispatch, then fail (a confirmed not-broadcast) — the void refunds in full.
	withdrawal_app::dispatch_withdrawal(h.withdrawals.as_ref(), &StubCustody, &h.notify, withdrawal.id())
		.await
		.unwrap();
	h.relay.drain().await;
	withdrawal_app::fail_withdrawal(h.withdrawals.as_ref(), &h.notify, withdrawal.id()).await.unwrap();
	h.relay.drain().await;

	// Inject the incident's parked `Dispatched` row for the now-failed withdrawal, then
	// "recover" it the wrong way: unpark it as if an operator ran the SQL after the fail.
	let event = WithdrawalEvent::Dispatched {
		withdrawal_id: withdrawal.id(),
		user,
		network,
		address: destination(network),
		amount: withdrawal.amount(),
		fee: withdrawal.fee(),
	};
	let event_id = Uuid::new_v4();
	sqlx::query("INSERT INTO outbox (event_id, aggregate, aggregate_id, kind, payload, parked_at, last_error) VALUES ($1, 'withdrawals', $2, 'withdrawals', $3::jsonb, now(), 'custody rejected: treasury underfunded on-chain (test)')")
		.bind(event_id)
		.bind(withdrawal.id().raw())
		.bind(serde_json::to_string(&event).unwrap())
		.execute(&h.pool)
		.await
		.expect("inject the parked Dispatched row");
	sqlx::query("UPDATE outbox SET parked_at = NULL, last_error = NULL WHERE event_id = $1")
		.bind(event_id)
		.execute(&h.pool)
		.await
		.expect("unpark the Dispatched row");

	// Drain with a counting custody: the guard must park the event again without a send.
	let broadcasts = Arc::new(AtomicUsize::new(0));
	let relay = Relay::new(h.pool.clone(), h.ledger.clone(), Arc::new(CountingCustody { broadcasts: broadcasts.clone() }), h.notify.clone());
	relay.drain().await;

	let (is_dispatched, is_parked, last_error): (bool, bool, Option<String>) =
		sqlx::query_as("SELECT dispatched_at IS NOT NULL, parked_at IS NOT NULL, last_error FROM outbox WHERE event_id = $1")
			.bind(event_id)
			.fetch_one(&h.pool)
			.await
			.expect("the re-parked row is still queryable");
	assert!(!is_dispatched, "the unparked Dispatched event must never be marked dispatched");
	assert!(is_parked, "the guard must park the event again");
	assert!(last_error.is_some_and(|e| e.contains("not processing")), "the park reason names the state guard");
	assert_eq!(broadcasts.load(Ordering::SeqCst), 0, "custody must never see a broadcast for a non-processing withdrawal");
}

/// A counting custody port adapter (no chain): every broadcast is recorded and refused, so
/// a guard regression is observable as both a call count and a park-not-dispatch.
struct CountingCustody {
	broadcasts: Arc<AtomicUsize>,
}

impl Gateway for CountingCustody {}

#[async_trait]
impl Custody for CountingCustody {
	async fn broadcast(&self, _request: &BroadcastRequest) -> Result<(), CustodyError> {
		self.broadcasts.fetch_add(1, Ordering::SeqCst);
		Err(CustodyError::Rejected("test custody refuses every broadcast".into()))
	}
}

/// Push a withdrawal's last transition past the reaper's abandonment window.
async fn backdate_withdrawal(pool: &PgPool, id: Uuid) {
	sqlx::query("UPDATE withdrawals SET updated_at = now() - interval '1 hour' WHERE id = $1")
		.bind(id)
		.execute(pool)
		.await
		.expect("backdate the withdrawal");
}
