use sqlx::postgres::{PgPool, PgPoolOptions};

/// Open a connection pool to Postgres. The pool is `Clone` and shared through
/// [`AppState`](crate::api::state::AppState); add migrations and repositories on
/// top of it as relational features land.
pub async fn connect(database_url: &str) -> anyhow::Result<PgPool> {
	let pool = PgPoolOptions::new().max_connections(10).connect(database_url).await?;
	Ok(pool)
}
