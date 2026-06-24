//! Liveness (`Check`) and readiness (`Readiness`) probes for the gRPC surface.
//!
//! `Check` is trivial and unauthenticated — "is the process up?" — and backs the
//! `cabinet` BFF smoke path (browser → BFF → gRPC). `Readiness` answers the distinct
//! question "can this instance actually move money?": it pings Postgres and TigerBeetle
//! and inspects the outbox, so an orchestrator/LB drains an instance that is up but
//! wedged (dead DB, unreachable ledger, parked rows, or a growing dispatch backlog)
//! instead of routing money-moving traffic to it. Both stay unauthenticated — they are
//! infrastructure probes, not data services (see [`services::serve`](super)).

use std::{sync::Arc, time::Duration};

use domain::balance::LedgerAccountKey;
use evbanking_contracts::banking::v1::{CheckRequest, CheckResponse, ReadinessRequest, ReadinessResponse, health_service_server::HealthService};
use sqlx::PgPool;
use tonic::{Request, Response, Status};

use crate::{infrastructure::outbox, ports::ledger::Ledger};

/// Readiness fails when the undispatched outbox backlog's oldest row is older than this —
/// a healthy relay drains sub-second, so a multi-minute-old head means it is wedged (a
/// stalled ledger, a held singleton lock) even though no row has parked yet.
const BACKLOG_AGE_LIMIT: Duration = Duration::from_secs(120);

/// Backs `HealthService`. Liveness needs nothing; readiness needs the control-plane pool
/// (for `SELECT 1` + the outbox scan) and the [`Ledger`] gateway (for a cheap TB ping).
#[derive(Clone)]
pub struct Health {
	pool: PgPool,
	ledger: Arc<dyn Ledger>,
}

impl Health {
	pub fn new(pool: PgPool, ledger: Arc<dyn Ledger>) -> Self {
		Self { pool, ledger }
	}
}

#[tonic::async_trait]
impl HealthService for Health {
	async fn check(&self, _request: Request<CheckRequest>) -> Result<Response<CheckResponse>, Status> {
		Ok(Response::new(CheckResponse { status: "ok".to_string() }))
	}

	async fn readiness(&self, _request: Request<ReadinessRequest>) -> Result<Response<ReadinessResponse>, Status> {
		// Postgres: a trivial round-trip on the request pool proves a live connection.
		let db_ok = sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(&self.pool).await.is_ok();

		// TigerBeetle: a cheap `lookup_accounts` on a seeded claims account (`Fund`). A
		// closed/stalled cluster surfaces as `Err` (the gateway bounds the call), so this
		// distinguishes "process up" from "ledger reachable".
		let ledger_ok = self.ledger.balance(&LedgerAccountKey::Fund).await.is_ok();

		// Outbox pipeline depth: any parked row needs operator intervention (BANK-FAULT-01),
		// and a stale undispatched head means a wedged relay. Either gates readiness.
		let depth = outbox::pipeline_depth(&self.pool).await.ok();
		let parked_rows = depth.as_ref().map_or(0, |d| d.parked.max(0) as u64);
		let backlog = depth.as_ref().map_or(0, |d| d.backlog.max(0) as u64);
		let oldest_backlog_age_secs = depth.as_ref().map_or(0, |d| d.oldest_backlog_age_secs.max(0) as u64);

		let ready = db_ok && ledger_ok && depth.is_some() && parked_rows == 0 && oldest_backlog_age_secs <= BACKLOG_AGE_LIMIT.as_secs();

		Ok(Response::new(ReadinessResponse {
			ready,
			db_ok,
			ledger_ok,
			parked_rows,
			backlog,
			oldest_backlog_age_secs,
		}))
	}
}
