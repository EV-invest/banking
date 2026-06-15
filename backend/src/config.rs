use std::{env, net::SocketAddr};

use anyhow::Context;

/// Application configuration, sourced from environment variables (and `.env`
/// in development via `dotenvy`).
#[derive(Clone, Debug)]
pub struct AppConfig {
	pub database_url: String,
	/// Axum HTTP listener address.
	pub bind_addr: SocketAddr,
	/// tonic gRPC listener address.
	pub grpc_addr: SocketAddr,
	pub sentry_dsn: Option<String>,
	pub app_env: String,
	/// TigerBeetle replica address (e.g. `"127.0.0.1:3001"` or `"3000"`).
	pub tigerbeetle_address: String,
	/// TigerBeetle cluster id. `0` for single-node dev.
	pub tigerbeetle_cluster_id: u128,
}

impl AppConfig {
	pub fn from_env() -> anyhow::Result<Self> {
		let database_url = env::var("DATABASE_URL").context("DATABASE_URL must be set")?;
		let bind_addr = env::var("BIND_ADDR")
			.unwrap_or_else(|_| "0.0.0.0:8080".to_string())
			.parse()
			.context("BIND_ADDR must be a valid socket address, e.g. 0.0.0.0:8080")?;
		let grpc_addr = env::var("GRPC_ADDR")
			.unwrap_or_else(|_| "0.0.0.0:50051".to_string())
			.parse()
			.context("GRPC_ADDR must be a valid socket address, e.g. 0.0.0.0:50051")?;
		let sentry_dsn = env::var("SENTRY_DSN").ok().filter(|s| !s.is_empty());
		let app_env = env::var("APP_ENV").unwrap_or_else(|_| "development".to_string());
		let tigerbeetle_address = env::var("TIGERBEETLE_ADDRESS").unwrap_or_else(|_| "127.0.0.1:3001".to_string());
		let tigerbeetle_cluster_id = env::var("TIGERBEETLE_CLUSTER_ID")
			.unwrap_or_else(|_| "0".to_string())
			.parse()
			.context("TIGERBEETLE_CLUSTER_ID must be an integer")?;
		Ok(Self {
			database_url,
			bind_addr,
			grpc_addr,
			sentry_dsn,
			app_env,
			tigerbeetle_address,
			tigerbeetle_cluster_id,
		})
	}
}
