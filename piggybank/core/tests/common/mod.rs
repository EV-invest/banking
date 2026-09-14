//! Shared bring-up for the integration tests: real Postgres **and** TigerBeetle, no
//! mocks. Every suite here opens the same two connections the same way, so the steps
//! live once instead of once per file.
//!
//! Each integration test is its own crate, so a suite that uses only part of this
//! module still compiles the rest — hence the blanket `dead_code` allowance.
#![allow(dead_code)]

use std::sync::{Arc, LazyLock};

use domain::users::UserId;
use piggybank_core::{
	infrastructure::{
		db,
		ledger::{self, TbLedger},
		tigerbeetle::TigerBeetle,
	},
	ports::ledger::Ledger,
};
use sqlx::PgPool;

/// Serializes the tests that own the relay as a *process* would: those that run
/// [`Relay::run`](piggybank_core::infrastructure::relay::Relay::run) or hold its
/// session-level outbox advisory lock (`acquire_outbox_lock`). The lock is one per
/// database, so two such tests in one binary would block each other on it — a driver
/// polling for "the relay applied my row" then times out while `run` is still queued
/// behind the sibling's lock. A test that only calls the unfenced `drain()` does not
/// take this: it never touches the lock.
///
/// Scope, honestly: a `LazyLock` lives in one process, and `cargo test` runs test
/// binaries one after another, so today this guards only tests that share a binary. A
/// runner that parallelizes binaries (nextest) is outside its reach — which is why
/// `relay_shutdown` also drains the shared backlog before it starts timing, and waits
/// far longer than a drain takes.
static RELAY: LazyLock<tokio::sync::Mutex<()>> = LazyLock::new(|| tokio::sync::Mutex::new(()));

/// Hold for the duration of a test that runs `Relay::run` or takes the outbox lock.
pub async fn relay_exclusive() -> tokio::sync::MutexGuard<'static, ()> {
	RELAY.lock().await
}

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

/// Mirror a concierge KYC tier onto a local user row.
///
/// Banking deliberately has no aggregate transition for `kyc_level` — the identity plane
/// owns the value and the lifecycle bridge is its only writer — so a test that needs a
/// verified investor sets the column exactly the way the bridge does. `provision` leaves
/// a fresh user at the schema default (tier 0, unverified), which is what the money gates
/// refuse.
pub async fn set_kyc_level(pool: &PgPool, user: UserId, level: i32) {
	let affected = sqlx::query("UPDATE users SET kyc_level = $2 WHERE id = $1")
		.bind(user.raw())
		.bind(level)
		.execute(pool)
		.await
		.expect("mirror the KYC tier")
		.rows_affected();
	assert_eq!(affected, 1, "no user row to mirror the KYC tier onto");
}
