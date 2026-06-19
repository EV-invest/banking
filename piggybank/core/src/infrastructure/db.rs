use sqlx::postgres::{PgPool, PgPoolOptions};

/// Open a connection pool to Postgres (the control plane). The pool is `Clone`
/// and shared through [`AppState`](crate::AppState); the repositories and event log
/// layer on top.
pub async fn connect(database_url: &str) -> anyhow::Result<PgPool> {
	let pool = PgPoolOptions::new().max_connections(10).connect(database_url).await?;
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
