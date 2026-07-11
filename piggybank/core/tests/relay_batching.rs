//! Integration test for the relay's batch-bounded drain (issue #38) — real Postgres
//! **and** TigerBeetle (no mocks, per the project rules). Runs when `DATABASE_URL` is
//! set and a TigerBeetle replica is reachable (`nix run .#db` + `.#tb`), and skips
//! otherwise.
//!
//! `drain()` now loops the per-batch `drain_batch` (which `run()` fences on the lock
//! connection between rows); this proves the loop still exhausts a backlog larger than
//! one batch (128 rows) — a regression here would silently stall a catch-up drain after
//! the first batch.

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
		deposits::PgDeposits,
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
	deposits: PgDeposits,
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
		eprintln!("TigerBeetle unreachable — skipping relay batching test");
		return None;
	}
	Some(Harness {
		deposits: PgDeposits::new(pool.clone()),
		pool,
		notify: Arc::new(Notify::new()),
	})
}

/// A backlog wider than one batch (129 > 128 rows) drains to exhaustion in a single
/// `drain()` call — the multi-batch continuation the batch-bounded refactor introduced.
#[tokio::test]
async fn drain_exhausts_a_backlog_wider_than_one_batch() {
	let Some(h) = harness().await else { return };
	let relay = Relay::new(h.pool.clone(), ledger_for(&h.pool), Arc::new(StubCustody), h.notify.clone());

	let party = Party::User(UserId::new());
	let claim = party.claim_key();
	let before = h.balance(&claim).await;
	for _ in 0..129 {
		balance_app::record_deposit(&h.deposits, &h.notify, tx_ref(), party.clone(), Network::Bep20, usdt("1"))
			.await
			.unwrap();
	}

	let throttle = relay.drain().await;

	assert!(!throttle, "a healthy multi-batch drain must not report a transient failure");
	assert_eq!(h.balance(&claim).await.saturating_sub(before), usdt("129").base_units(), "every row past the first batch applied");
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
