//! Integration test for the relay's singleton invariant (BANK-FAULT-03/06) — real
//! Postgres **and** TigerBeetle (no mocks, per the project rules). Runs when
//! `DATABASE_URL` is set and a TigerBeetle replica is reachable (`nix run .#db` +
//! `.#tb`), and skips otherwise.
//!
//! The relay's whole ordering/atomicity argument (strict `seq`, reserve-before-complete,
//! the settle-time liquidity pre-check) is valid only with exactly one drainer. This
//! proves that invariant is now *enforced* by a fixed-key session advisory lock: a second
//! Relay against the same Postgres cannot take the lock while the first holds it, so it
//! would block in `run()` rather than concurrently draining the same outbox.

use std::sync::Arc;

use domain::{
	balance::{LedgerAccountKey, Party},
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
use uuid::Uuid;

struct Harness {
	pool: PgPool,
	notify: Arc<Notify>,
}
impl Harness {
	async fn balance(&self, key: &LedgerAccountKey) -> u128 {
		ledger_for(&self.pool).balance(key).await.unwrap().posted
	}
}

async fn harness() -> Option<Harness> {
	let url = std::env::var("DATABASE_URL").ok().filter(|s| !s.is_empty())?;
	let pool = db::connect(&url).await.expect("connect to Postgres");
	db::migrate(&pool).await.expect("apply migrations");

	if ledger::seed_singletons(ledger_for(&pool).as_ref()).await.is_err() {
		eprintln!("TigerBeetle unreachable — skipping relay singleton test");
		return None;
	}
	Some(Harness {
		pool,
		notify: Arc::new(Notify::new()),
	})
}

fn relay(h: &Harness) -> Relay {
	Relay::new(h.pool.clone(), ledger_for(&h.pool), Arc::new(StubCustody), h.notify.clone())
}

/// Two relays, one Postgres: the first takes the outbox lock, the second cannot — so a
/// rolling deploy / extra replica blocks on the lock instead of double-draining. After
/// the holder's connection closes (as on process exit), the standby takes over. The lock
/// holder can still drain a committed event while it owns the lock.
#[tokio::test]
async fn only_one_relay_drains_under_the_outbox_lock() {
	let Some(h) = harness().await else { return };
	let primary = relay(&h);
	let standby = relay(&h);

	// The primary takes the singleton lock (its held connection *is* the lock).
	let lock = primary.acquire_outbox_lock().await.expect("primary takes the outbox lock");

	// A second instance cannot take it — it would block in `run()` and never drain.
	let contender = standby.try_acquire_outbox_lock().await.expect("query the lock");
	assert!(contender.is_none(), "a second relay must not acquire the outbox lock while one is held");

	// The holder still drains: commit a deposit and prove the primary moves it in TB.
	let party = Party::User(UserId::new());
	let claim = party.claim_key();
	let before = h.balance(&claim).await;
	balance_app::record_deposit(&h.pool, &h.notify, tx_ref(), party, Network::Bep20, usdt("250")).await.unwrap();
	primary.drain().await;
	let after = h.balance(&claim).await;
	assert_eq!(after.saturating_sub(before), usdt("250").base_units(), "the lock holder applied the committed deposit");

	// Close the holding connection — as a process exit does in production — releasing the
	// session lock so a standby can take over without coordination.
	lock.close().await.expect("close the lock-holding connection");
	let taken = standby.try_acquire_outbox_lock().await.expect("query the lock");
	assert!(taken.is_some(), "once the holder's connection closes, a standby acquires the lock");
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
