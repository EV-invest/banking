//! Integration test for the relay's graceful shutdown (BANK-FAULT-02) — real Postgres
//! **and** TigerBeetle (no mocks, per the project rules). Runs when `DATABASE_URL` is set
//! and a TigerBeetle replica is reachable (`nix run .#db` + `.#tb`), and skips otherwise.
//!
//! Before this fix `Relay::run` looped forever and the composition root's `select!` simply
//! aborted it on shutdown — a deploy/restart could tear the relay down mid-`dispatch`. It
//! now takes a `CancellationToken` and observes it only at the wait point *between* drains,
//! so the current iteration always finishes (already crash-safe between rows) and the
//! process winds down cleanly instead of being killed at an arbitrary `await`.
//!
//! This lives in its own test binary (own process) so its long-lived relay's session
//! advisory lock never overlaps the singleton test's lock assertions on a shared DB.

use std::{sync::Arc, time::Duration};

use domain::{
	balance::Party,
	money::{Network, TxRef, Usdt},
	users::UserId,
};
use piggybank_core::{
	application::balance as balance_app,
	infrastructure::{
		custody::StubCustody,
		db,
		ledger::{self, TbLedger},
		relay::Relay,
		tigerbeetle::TigerBeetle,
	},
	ports::ledger::Ledger,
};
use sqlx::PgPool;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

struct Harness {
	pool: PgPool,
	notify: Arc<Notify>,
}
impl Harness {
	async fn claim_balance(&self, party: &Party) -> u128 {
		ledger_for(&self.pool).balance(&party.claim_key()).await.unwrap().posted
	}
}

async fn harness() -> Option<Harness> {
	let url = std::env::var("DATABASE_URL").ok().filter(|s| !s.is_empty())?;
	let pool = db::connect(&url).await.expect("connect to Postgres");
	db::migrate(&pool).await.expect("apply migrations");
	if ledger::seed_singletons(ledger_for(&pool).as_ref()).await.is_err() {
		eprintln!("TigerBeetle unreachable — skipping relay shutdown test");
		return None;
	}
	Some(Harness {
		pool,
		notify: Arc::new(Notify::new()),
	})
}

/// A running `Relay::run` finishes its current drain iteration and then exits cleanly when
/// its `CancellationToken` is cancelled — it is never torn down mid-`drain`, and it does not
/// loop forever ignoring the token. Run `run` concurrently (structured `join!`, no detached
/// spawn) with a driver that commits a deposit, waits until the relay has applied it (the
/// iteration completed), then cancels the token. `join!` only returns once `run` has itself
/// observed the cancellation and exited; the outer timeout proves it does not loop forever.
#[tokio::test]
async fn run_finishes_its_drain_then_stops_on_cancellation() {
	let Some(h) = harness().await else { return };
	let relay = Relay::new(h.pool.clone(), ledger_for(&h.pool), Arc::new(StubCustody), h.notify.clone());

	let party = Party::User(UserId::new());
	let before = h.claim_balance(&party).await;
	balance_app::record_deposit(&h.pool, &h.notify, tx_ref(), party.clone(), Network::Bep20, usdt("125"))
		.await
		.unwrap();
	let expected = before.saturating_add(usdt("125").base_units());

	let shutdown = CancellationToken::new();
	let driver = drive_until_applied_then_cancel(&h, &party, expected, shutdown.clone());

	// `join!` returns only after `run` exits; the timeout guards against a regression that
	// ignored the token and looped forever (the driver always cancels, so a correct run ends).
	let (applied, ()) = tokio::time::timeout(Duration::from_secs(15), async { tokio::join!(driver, relay.run(shutdown)) })
		.await
		.expect("run returns promptly after the current drain iteration on cancellation");
	assert_eq!(applied, expected, "the relay applied the committed deposit before shutdown");
}

/// Poll until the relay has applied the committed deposit (proving the in-flight drain
/// iteration ran to completion while the token was still live), then cancel the token so the
/// relay observes shutdown at its wait point and exits. Returns the observed balance.
async fn drive_until_applied_then_cancel(h: &Harness, party: &Party, expected: u128, shutdown: CancellationToken) -> u128 {
	let mut applied = h.claim_balance(party).await;
	for _ in 0..100 {
		if applied == expected {
			break;
		}
		tokio::time::sleep(Duration::from_millis(100)).await;
		applied = h.claim_balance(party).await;
	}
	shutdown.cancel();
	applied
}

fn ledger_for(pool: &PgPool) -> Arc<dyn Ledger> {
	let address = std::env::var("TIGERBEETLE_ADDRESS").unwrap_or_else(|_| "127.0.0.1:3033".to_owned());
	let cluster = std::env::var("TIGERBEETLE_CLUSTER_ID").ok().and_then(|s| s.parse().ok()).unwrap_or(0u128);
	let tigerbeetle = Arc::new(TigerBeetle::connect(cluster, &address).expect("connect to TigerBeetle"));
	Arc::new(TbLedger::new(tigerbeetle, pool.clone()))
}

fn usdt(decimal: &str) -> Usdt {
	Usdt::parse_decimal(decimal).unwrap()
}

fn tx_ref() -> TxRef {
	TxRef::parse(&format!("itest-{}", Uuid::new_v4())).unwrap()
}
