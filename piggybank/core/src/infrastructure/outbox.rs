//! Transactional outbox — the write side and the drain side.
//!
//! Domain events drained from an aggregate are written to the append-only
//! `event_log` (always, the audit trail) and, for events the relay must act on
//! (money/saga moves), to the `outbox` **inside the same transaction** as the state
//! change — the one ACID point. Each event gets a fresh `event_id` UUID, the stable
//! idempotency key the relay derives deterministic TigerBeetle transfer ids from
//! (never the delivery cursor `seq`). [`super::relay`] drains undispatched rows in
//! strict `seq` order.

use domain::{
	architecture::{DomainEvent, EmitsEvents, Entity, Identifier},
	error::DomainError,
};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

/// Take the per-user write lock — a transaction-scoped `pg_advisory_xact_lock` keyed on the
/// user id — inside an open command transaction, held until commit. Every flow that spends a
/// user's unified `UserClaim` (withdraw, subscribe) takes this *same* target before its
/// Read-First so the optimistic solvency checks serialize across flows: without it a
/// concurrent withdraw + subscribe on one claim both read the same stale `available()`, both
/// commit `Queued`, and the second reserve then parks in the relay, silently diverging PG
/// from TB. (Redemptions serialize on their `fund_positions` row instead — a different
/// account, the units, not the claim.) An advisory lock (over a `users`-row `FOR UPDATE`)
/// engages even before the user row is materialized and needs no FK, so the serialization is
/// unconditional. TigerBeetle's non-negative flag remains the actual money backstop.
pub async fn lock_user(conn: &mut PgConnection, user_id: Uuid) -> Result<(), DomainError> {
	sqlx::query("SELECT pg_advisory_xact_lock($1)")
		.bind(advisory_key(user_id))
		.execute(&mut *conn)
		.await
		.map_err(repo_err)?;
	Ok(())
}

/// Insert one event into the `event_log` (always) and the `outbox` (when `relay`),
/// under a caller-supplied `event_id` so the two rows share the idempotency key.
/// Used directly for standalone ledger facts (deposits/seeding) that have no
/// aggregate to drain.
pub async fn insert_event(conn: &mut PgConnection, event_id: Uuid, aggregate: &str, aggregate_id: Uuid, kind: &str, payload: &str, relay: bool) -> Result<(), DomainError> {
	sqlx::query("INSERT INTO event_log (event_id, aggregate, aggregate_id, kind, payload) VALUES ($1, $2, $3, $4, $5::jsonb)")
		.bind(event_id)
		.bind(aggregate)
		.bind(aggregate_id)
		.bind(kind)
		.bind(payload)
		.execute(&mut *conn)
		.await
		.map_err(repo_err)?;
	if relay {
		sqlx::query("INSERT INTO outbox (event_id, aggregate, aggregate_id, kind, payload) VALUES ($1, $2, $3, $4, $5::jsonb)")
			.bind(event_id)
			.bind(aggregate)
			.bind(aggregate_id)
			.bind(kind)
			.bind(payload)
			.execute(&mut *conn)
			.await
			.map_err(repo_err)?;
	}
	Ok(())
}
/// Drain an aggregate's pending events onto the open transaction: each event lands
/// in the `event_log`, and in the `outbox` too when `relay` is true (money-moving
/// aggregates). State and events therefore commit together or not at all.
pub async fn drain_to_outbox<A>(conn: &mut PgConnection, aggregate: &mut A, relay: bool) -> Result<(), DomainError>
where
	A: EmitsEvents,
	<A as Entity>::Id: Identifier<Underlying = Uuid>, {
	let aggregate_id = Entity::id(aggregate).underlying();
	for event in aggregate.drain_events() {
		let payload = serde_json::to_string(&event).map_err(|e| DomainError::Repository(e.to_string()))?;
		insert_event(conn, Uuid::new_v4(), A::NAME, aggregate_id, <A::Event as DomainEvent>::KIND, &payload, relay).await?;
	}
	Ok(())
}
/// An undispatched outbox row, ready for the relay. `payload` is the event JSON read
/// back as text (the workspace `sqlx` has no `json` feature, so JSONB is cast to text).
#[derive(sqlx::FromRow)]
pub struct OutboxRow {
	pub seq: i64,
	pub event_id: Uuid,
	pub aggregate: String,
	pub aggregate_id: Uuid,
	pub kind: String,
	pub payload: String,
	pub attempts: i32,
}
/// The next drainable events in strict `seq` order: neither dispatched nor parked. No
/// `SKIP LOCKED`: the relay is a lock-enforced singleton (`pg_advisory_lock`, see
/// [`super::relay::Relay::run`]), so the order is total and a reservation's pending always
/// precedes its completion — `SKIP LOCKED` would let disjoint workers break that, so it is
/// deliberately omitted. A *parked* row stays in the table (queryable by reconciliation /
/// the reaper) but is excluded here so one non-retryable event can't wedge the queue.
pub async fn next_batch(pool: &PgPool, limit: i64) -> Result<Vec<OutboxRow>, sqlx::Error> {
	sqlx::query_as::<_, OutboxRow>(
		"SELECT seq, event_id, aggregate, aggregate_id, kind, payload::text AS payload, attempts FROM outbox WHERE dispatched_at IS NULL AND parked_at IS NULL ORDER BY seq LIMIT $1",
	)
	.bind(limit)
	.fetch_all(pool)
	.await
}
/// Mark a row applied to the ledger (the relay advances past it).
pub async fn mark_dispatched(pool: &PgPool, seq: i64) -> Result<(), sqlx::Error> {
	sqlx::query("UPDATE outbox SET dispatched_at = now() WHERE seq = $1").bind(seq).execute(pool).await?;
	Ok(())
}
/// Move a non-retryable row to the distinct **parked** terminal state: stamp `parked_at`
/// (NOT `dispatched_at`), bump the attempt counter, and store the reason. The row stays
/// queryable for reconciliation/the reaper and an operator, yet is excluded from
/// [`next_batch`] so it never wedges the single-worker drain. The relay still advances
/// past it in-memory; this is what replaces the old "park == mark_dispatched" drop.
pub async fn mark_parked(pool: &PgPool, seq: i64, reason: &str) -> Result<(), sqlx::Error> {
	sqlx::query("UPDATE outbox SET parked_at = now(), attempts = attempts + 1, last_error = $2 WHERE seq = $1")
		.bind(seq)
		.bind(reason)
		.execute(pool)
		.await?;
	Ok(())
}
/// Stamp that a parked multi-leg event has been flagged for compensation (it left the
/// ledger half-applied), so reconciliation can tell a still-open park from one already
/// routed to its recovery path.
pub async fn mark_compensated(pool: &PgPool, seq: i64) -> Result<(), sqlx::Error> {
	sqlx::query("UPDATE outbox SET compensated_at = now() WHERE seq = $1").bind(seq).execute(pool).await?;
	Ok(())
}
/// Record a transient (retryable) failure for forensics without advancing or parking —
/// bumps the attempt counter and stores the last error.
pub async fn record_failure(pool: &PgPool, seq: i64, error: &str) -> Result<(), sqlx::Error> {
	sqlx::query("UPDATE outbox SET attempts = attempts + 1, last_error = $2 WHERE seq = $1")
		.bind(seq)
		.bind(error)
		.execute(pool)
		.await?;
	Ok(())
}
/// A parked outbox row, surfaced to reconciliation and the operator console: enough to
/// identify the stranded saga (`aggregate`/`aggregate_id`), why it parked (`last_error`),
/// when (`parked_at_unix`), and whether it has already been compensated.
#[derive(sqlx::FromRow)]
pub struct ParkedRow {
	pub seq: i64,
	pub event_id: Uuid,
	pub aggregate: String,
	pub aggregate_id: Uuid,
	pub kind: String,
	pub last_error: Option<String>,
	pub parked_at_unix: i64,
	pub compensated: bool,
}
/// Scan every parked row (newest first) — the reconciliation job's parked-row leg. Cheap
/// against the partial `outbox_parked_idx`; parked rows are an exceptional set.
pub async fn parked_rows(pool: &PgPool) -> Result<Vec<ParkedRow>, sqlx::Error> {
	sqlx::query_as::<_, ParkedRow>(
		"SELECT seq, event_id, aggregate, aggregate_id, kind, last_error, EXTRACT(EPOCH FROM parked_at)::bigint AS parked_at_unix, compensated_at IS NOT NULL AS compensated \
		 FROM outbox WHERE parked_at IS NOT NULL ORDER BY parked_at DESC",
	)
	.fetch_all(pool)
	.await
}
/// Clear a parked row so the relay re-drives it — the operator's intervention once the
/// park's cause (an underfunded treasury, a rejected broadcast) is fixed. `attempts`
/// MUST reset with it: the relay re-parks a bounded retryable at `MAX_RETRYABLE_ATTEMPTS`,
/// so a retry-exhausted row would otherwise re-park on its first redelivery. `last_error`
/// is kept for forensics (the next attempt overwrites it). Refuses a dispatched row
/// (nothing to re-drive) and a **compensated** one — its recovery event already applied,
/// so re-driving would double-apply. Returns whether a row was unparked.
pub async fn unpark(pool: &PgPool, seq: i64) -> Result<bool, sqlx::Error> {
	let result = sqlx::query("UPDATE outbox SET parked_at = NULL, attempts = 0 WHERE seq = $1 AND parked_at IS NOT NULL AND dispatched_at IS NULL AND compensated_at IS NULL")
		.bind(seq)
		.execute(pool)
		.await?;
	Ok(result.rows_affected() > 0)
}
/// One row's terminal stamps — `(dispatched, compensated)` — read after a refused
/// [`unpark`] so the caller can answer precisely (compensated vs dispatched vs not
/// parked / unknown seq).
pub async fn unpark_refusal(pool: &PgPool, seq: i64) -> Result<Option<(bool, bool)>, sqlx::Error> {
	sqlx::query_as::<_, (bool, bool)>("SELECT dispatched_at IS NOT NULL, compensated_at IS NOT NULL FROM outbox WHERE seq = $1")
		.bind(seq)
		.fetch_optional(pool)
		.await
}
/// The relay's pipeline depth, for the readiness probe: how many rows the relay has parked
/// (`parked_at IS NOT NULL` — the canonical parked predicate, NOT `last_error IS NOT NULL`,
/// which also matches a transiently-retried-but-still-live row), how many are still
/// undispatched (`dispatched_at IS NULL AND parked_at IS NULL`, the drain predicate), and the
/// age in seconds of the oldest such row (a wedged relay shows a growing backlog age). One
/// round-trip; both counts ride the partial indexes.
#[derive(sqlx::FromRow)]
pub struct PipelineDepth {
	pub parked: i64,
	pub backlog: i64,
	pub oldest_backlog_age_secs: i64,
}
pub async fn pipeline_depth(pool: &PgPool) -> Result<PipelineDepth, sqlx::Error> {
	sqlx::query_as::<_, PipelineDepth>(
		"SELECT \
		   COUNT(*) FILTER (WHERE parked_at IS NOT NULL) AS parked, \
		   COUNT(*) FILTER (WHERE dispatched_at IS NULL AND parked_at IS NULL) AS backlog, \
		   COALESCE(EXTRACT(EPOCH FROM now() - MIN(occurred_at) FILTER (WHERE dispatched_at IS NULL AND parked_at IS NULL)), 0)::bigint AS oldest_backlog_age_secs \
		 FROM outbox",
	)
	.fetch_one(pool)
	.await
}
/// Fold a user UUID into the `bigint` advisory-lock key space. XOR-ing the two 64-bit halves
/// keeps the full 128 bits of entropy in play (a single half would ignore the other), so two
/// distinct users practically never share a key and serialize against each other.
fn advisory_key(user_id: Uuid) -> i64 {
	let (hi, lo) = user_id.as_u64_pair();
	(hi ^ lo) as i64
}

fn repo_err(err: sqlx::Error) -> DomainError {
	DomainError::Repository(err.to_string())
}
