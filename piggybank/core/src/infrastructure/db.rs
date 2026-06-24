use sqlx::postgres::{PgPool, PgPoolOptions};

/// Open a connection pool to Postgres (the control plane) with the sqlx-default size
/// (10). The pool is `Clone` and shared through [`AppState`](crate::AppState); the
/// repositories and event log layer on top. sqlx 0.9 already applies sane
/// `acquire_timeout`/`idle_timeout`/`max_lifetime` defaults, so size is the only knob.
pub async fn connect(database_url: &str) -> anyhow::Result<PgPool> {
	connect_sized(database_url, 10).await
}

/// Open a Postgres pool with an explicit `max_connections`. The composition root sizes
/// the request-serving pool from config and gives the outbox relay its own small pool, so
/// a burst of read traffic and money dispatch can't exhaust each other's connections.
pub async fn connect_sized(database_url: &str, max_connections: u32) -> anyhow::Result<PgPool> {
	let pool = PgPoolOptions::new().max_connections(max_connections).connect(database_url).await?;
	Ok(pool)
}

/// Apply pending control-plane migrations (embedded from `piggybank/core/migrations`
/// at build time) on startup; also used by integration tests for a hermetic schema.
/// Idempotent. Author new migration FILES with the sqlx CLI
/// (`sqlx migrate add --source piggybank/core/migrations --sequential <name>`),
/// never by hand; the embedded runner here is interoperable with the CLI (same
/// `_sqlx_migrations` table).
pub async fn migrate(pool: &PgPool) -> anyhow::Result<()> {
	sqlx::migrate!().run(pool).await?;
	Ok(())
}
