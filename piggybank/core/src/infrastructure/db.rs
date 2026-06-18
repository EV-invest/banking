use sqlx::postgres::{PgPool, PgPoolOptions};

/// Open a connection pool to Postgres (the control plane). The pool is `Clone`
/// and shared through [`AppState`](crate::AppState); the `UnitOfWork`,
/// repositories, event log, and projections layer on top as features land.
pub async fn connect(database_url: &str) -> anyhow::Result<PgPool> {
	let pool = PgPoolOptions::new().max_connections(10).connect(database_url).await?;
	Ok(pool)
}
