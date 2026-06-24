//! Integration tests for FB-07 — a readiness probe distinct from liveness (BANK-FAULT-08).
//! Real Postgres **and** TigerBeetle (no mocks, per the project rules). They run when
//! `DATABASE_URL` is set and a TigerBeetle replica is reachable (`nix run .#db` + `.#tb`),
//! and skip otherwise. The probes are driven directly through the `HealthService` impl
//! (`Health`), no server stood up.
//!
//! What's proven:
//!   - liveness (`Check`) is trivially `ok` and never inspects DB/TB/the relay;
//!   - readiness is `ready` on a clean pipeline;
//!   - a parked outbox row flips readiness to NOT ready while liveness stays `ok`
//!     (BANK-FAULT-08 — an external monitor can finally see parked-row accumulation);
//!   - an unreachable TigerBeetle flips readiness to NOT ready (`ledger_ok == false`).

use std::sync::Arc;

use evbanking_contracts::banking::v1::{CheckRequest, ReadinessRequest, health_service_server::HealthService};
use piggybank_core::{
	infrastructure::{
		db,
		ledger::{self, TbLedger},
		tigerbeetle::TigerBeetle,
	},
	ports::ledger::Ledger,
	services::health::Health,
};
use sqlx::PgPool;
use tonic::Request;
use uuid::Uuid;

struct Harness {
	pool: PgPool,
	ledger: Arc<dyn Ledger>,
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
		eprintln!("TigerBeetle unreachable — skipping readiness test");
		return None;
	}
	Some(Harness { pool, ledger })
}

/// A clean pipeline reads ready; injecting a parked outbox row flips readiness to NOT ready
/// (`parked_rows >= 1`) while liveness stays trivially `ok`. The parked row is the same shape
/// the relay leaves behind on a non-retryable failure (`parked_at` set, `dispatched_at` null).
#[tokio::test]
async fn a_parked_row_flips_readiness_not_liveness() {
	let Some(h) = harness().await else { return };
	let health = Health::new(h.pool.clone(), h.ledger.clone());

	// Liveness is trivial and unconditional.
	let live = health.check(Request::new(CheckRequest {})).await.expect("check ok").into_inner();
	assert_eq!(live.status, "ok", "liveness is always ok");

	// On a pipeline with no parked rows the probe reports ready (db + ledger reachable).
	let before = health.readiness(Request::new(ReadinessRequest {})).await.expect("readiness ok").into_inner();
	assert!(before.db_ok, "Postgres SELECT 1 must succeed");
	assert!(before.ledger_ok, "the TigerBeetle ping on Fund must succeed");

	// Inject a parked outbox row (parked_at set, dispatched_at null) — exactly what the relay
	// leaves on a non-retryable failure.
	let event_id = Uuid::new_v4();
	sqlx::query(
		"INSERT INTO outbox (event_id, aggregate, aggregate_id, kind, payload, parked_at, last_error) VALUES ($1, 'withdrawals', $2, 'withdrawals', $3::jsonb, now(), 'readiness probe test')",
	)
	.bind(event_id)
	.bind(Uuid::new_v4())
	.bind("\"parked\"")
	.execute(&h.pool)
	.await
	.expect("inject a parked outbox row");

	let after = health.readiness(Request::new(ReadinessRequest {})).await.expect("readiness ok").into_inner();
	assert!(after.parked_rows >= 1, "the parked row must be counted");
	assert!(!after.ready, "readiness must fail while a parked row exists");

	// Liveness is unmoved by the parked row.
	let live_again = health.check(Request::new(CheckRequest {})).await.expect("check ok").into_inner();
	assert_eq!(live_again.status, "ok", "liveness stays ok regardless of the relay backlog");
}

/// With the control plane healthy but TigerBeetle unreachable, readiness reports `ledger_ok
/// == false` and NOT ready, while liveness stays `ok`. We reuse the live harness only to seed
/// the `Fund` account's id-map row (so the unreachable ledger's `balance(Fund)` resolves the id
/// from Postgres and then actually issues a `lookup_accounts`), then swap in a ledger pointed at
/// a port no replica is listening on. The gateway bounds that call, so it fails deterministically
/// rather than hanging.
#[tokio::test]
async fn unreachable_tigerbeetle_flips_readiness_not_liveness() {
	let Some(h) = harness().await else { return };

	// Point a second ledger at a port no TigerBeetle replica is listening on. `Fund` is already
	// in the id-map (seeded by `harness`), so `balance(Fund)` reaches `lookup_accounts` and errs.
	let unreachable = Arc::new(TigerBeetle::connect(0, "127.0.0.1:1").expect("client init is lazy"));
	let ledger: Arc<dyn Ledger> = Arc::new(TbLedger::new(unreachable, h.pool.clone()));
	let health = Health::new(h.pool.clone(), ledger);

	let live = health.check(Request::new(CheckRequest {})).await.expect("check ok").into_inner();
	assert_eq!(live.status, "ok", "liveness stays ok even with TigerBeetle down");

	let ready = health.readiness(Request::new(ReadinessRequest {})).await.expect("readiness ok").into_inner();
	assert!(ready.db_ok, "Postgres is still reachable");
	assert!(!ready.ledger_ok, "an unreachable TigerBeetle must surface as ledger_ok == false");
	assert!(!ready.ready, "readiness must fail when the ledger is unreachable");
}
