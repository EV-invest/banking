//! Shared bring-up for the integration tests: real Postgres **and** TigerBeetle, no
//! mocks. Every suite here opens the same two connections the same way, so the steps
//! live once instead of once per file.
//!
//! Each integration test is its own crate, so a suite that uses only part of this
//! module still compiles the rest — hence the blanket `dead_code` allowance.
#![allow(dead_code)]

use std::sync::Arc;

use piggybank_core::{
	infrastructure::{
		db,
		ledger::{self, TbLedger},
		tigerbeetle::TigerBeetle,
	},
	ports::ledger::Ledger,
};
use sqlx::PgPool;

/// A migrated pool, or `None` when `DATABASE_URL` is unset — the signal every suite
/// uses to skip rather than fail on a machine without `nix run .#db`.
pub async fn pool() -> Option<PgPool> {
	let url = std::env::var("DATABASE_URL").ok().filter(|s| !s.is_empty())?;
	let pool = db::connect(&url).await.expect("connect to Postgres");
	db::migrate(&pool).await.expect("apply migrations");
	Some(pool)
}

/// A ledger over the configured TigerBeetle replica. Connecting is lazy on the
/// replica's side, so this succeeds even when nothing is listening — call
/// [`seeded_ledger`] when the test needs a replica that actually answers.
pub fn ledger_for(pool: &PgPool) -> Arc<dyn Ledger> {
	let address = std::env::var("TIGERBEETLE_ADDRESS").unwrap_or_else(|_| "127.0.0.1:3033".to_owned());
	let cluster = std::env::var("TIGERBEETLE_CLUSTER_ID").ok().and_then(|s| s.parse().ok()).unwrap_or(0u128);
	let tigerbeetle = Arc::new(TigerBeetle::connect(cluster, &address).expect("connect to TigerBeetle"));
	Arc::new(TbLedger::new(tigerbeetle, pool.clone()))
}

/// A ledger whose singleton accounts exist, or `None` when the replica is unreachable.
/// `skipping` names the suite in the skip notice.
pub async fn seeded_ledger(pool: &PgPool, skipping: &str) -> Option<Arc<dyn Ledger>> {
	let ledger = ledger_for(pool);
	if ledger::seed_singletons(ledger.as_ref()).await.is_err() {
		eprintln!("TigerBeetle unreachable — skipping {skipping}");
		return None;
	}
	Some(ledger)
}
