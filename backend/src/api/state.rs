use std::sync::Arc;

use sqlx::postgres::PgPool;

use crate::infrastructure::tigerbeetle::TigerBeetle;

/// Shared application state injected into handlers via Axum's `State` extractor.
/// Holds the wired infrastructure handles; cloning is cheap (a `PgPool` clone is
/// an `Arc` bump, and the TigerBeetle client is already behind an `Arc`).
#[derive(Clone)]
pub struct AppState {
	pub pool: PgPool,
	pub tigerbeetle: Arc<TigerBeetle>,
}

impl AppState {
	pub fn new(pool: PgPool, tigerbeetle: Arc<TigerBeetle>) -> Self {
		Self { pool, tigerbeetle }
	}
}
