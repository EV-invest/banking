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
/// The next undispatched events in strict `seq` order (a single relay worker, so no
/// `SKIP LOCKED` — order is total, and a reservation's pending always precedes its
/// completion).
pub async fn next_batch(pool: &PgPool, limit: i64) -> Result<Vec<OutboxRow>, sqlx::Error> {
	sqlx::query_as::<_, OutboxRow>("SELECT seq, event_id, aggregate, aggregate_id, kind, payload::text AS payload, attempts FROM outbox WHERE dispatched_at IS NULL ORDER BY seq LIMIT $1")
		.bind(limit)
		.fetch_all(pool)
		.await
}
/// Mark a row applied to the ledger (the relay advances past it).
pub async fn mark_dispatched(pool: &PgPool, seq: i64) -> Result<(), sqlx::Error> {
	sqlx::query("UPDATE outbox SET dispatched_at = now() WHERE seq = $1").bind(seq).execute(pool).await?;
	Ok(())
}
/// Record a (transient or parked) failure for forensics without advancing — bumps
/// the attempt counter and stores the last error.
pub async fn record_failure(pool: &PgPool, seq: i64, error: &str) -> Result<(), sqlx::Error> {
	sqlx::query("UPDATE outbox SET attempts = attempts + 1, last_error = $2 WHERE seq = $1")
		.bind(seq)
		.bind(error)
		.execute(pool)
		.await?;
	Ok(())
}
fn repo_err(err: sqlx::Error) -> DomainError {
	DomainError::Repository(err.to_string())
}
