//! Money-plane operations mode — the global read-only kill-switch.
//!
//! A single-row config flag (`operations_mode`) toggled by an admin RPC and checked at
//! every user money mutation (see [`support::unfrozen_caller`](crate::services::support)).
//! When `read_only` is on, withdraws/subscribes/redeems are refused — "pause deposits &
//! withdrawals". Plain config, not a domain aggregate, so it lives in the adapter layer.

use sqlx::PgPool;

/// Whether the money plane is in read-only mode. A missing row (should never happen —
/// migration seeds it) reads as NOT read-only, matching the default.
pub async fn is_read_only(pool: &PgPool) -> Result<bool, sqlx::Error> {
	let value: Option<bool> = sqlx::query_scalar("SELECT read_only FROM operations_mode WHERE id = TRUE").fetch_optional(pool).await?;
	Ok(value.unwrap_or(false))
}

/// Set read-only mode. Returns the value now in effect.
pub async fn set_read_only(pool: &PgPool, read_only: bool) -> Result<bool, sqlx::Error> {
	sqlx::query("UPDATE operations_mode SET read_only = $1, updated_at = now() WHERE id = TRUE")
		.bind(read_only)
		.execute(pool)
		.await?;
	Ok(read_only)
}
