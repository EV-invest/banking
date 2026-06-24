use std::{env, net::SocketAddr};

use anyhow::Context;

/// Default gRPC binds: loopback, so the hub's internal data/auth seams are not exposed on
/// every interface. A wider bind is an explicit opt-in that requires network segmentation
/// (see `docs/ARCHITECTURE.md`).
const DEFAULT_GRPC_ADDR: &str = "127.0.0.1:50051";
const DEFAULT_AUTH_GRPC_ADDR: &str = "127.0.0.1:50052";

/// Application configuration, sourced from environment variables (and `.env`
/// in development via `dotenvy`).
#[derive(Clone, Debug)]
pub struct AppConfig {
	pub database_url: String,
	/// Core tonic gRPC listener address (the hub's data-plane services).
	pub grpc_addr: SocketAddr,
	/// Auth service gRPC listener address (token issuance routes for clients).
	pub auth_grpc_addr: SocketAddr,
	pub sentry_dsn: Option<String>,
	/// PostHog project key for native product-analytics capture. `None` disables
	/// capture (a silent no-op), so the same code runs unconfigured (local, CI).
	pub posthog_key: Option<String>,
	/// PostHog ingestion host; `None` falls back to the library default
	/// (`https://us.i.posthog.com`).
	pub posthog_host: Option<String>,
	pub app_env: String,
	/// TigerBeetle replica address (e.g. `"127.0.0.1:3033"` or a bare `"3033"`).
	pub tigerbeetle_address: String,
	/// TigerBeetle cluster id. `0` for single-node dev.
	pub tigerbeetle_cluster_id: u128,
	/// Hub user ids (UUIDs) allowed to call admin RPCs (`RevokeTokens`/`DisableUser`).
	/// A coarse, config-driven allowlist standing in for RBAC until a role slice
	/// lands; `ADMIN_SUBJECTS` is a comma-separated list (empty ⇒ no admins).
	pub admin_subjects: Vec<String>,
	/// Endpoint of the separate-process signer (the key vault), for deposit-address
	/// provisioning over the `signer.v1` gRPC seam. The hub connects lazily, so this
	/// only needs to resolve by the time the first address is provisioned.
	pub signer_grpc_addr: String,
	/// Max connections for the request-serving Postgres pool (the core gRPC handlers).
	/// `DB_MAX_CONNECTIONS`; defaults to the sqlx default (10) — raise it for production.
	pub db_max_connections: u32,
	/// Max connections for the outbox relay's own dedicated Postgres pool, so request
	/// traffic and money dispatch can't exhaust each other. `RELAY_DB_MAX_CONNECTIONS`;
	/// a small pool suffices since the relay is a single-worker drainer (one drain
	/// connection + the lock-holding connection).
	pub relay_db_max_connections: u32,
}

impl AppConfig {
	pub fn from_env() -> anyhow::Result<Self> {
		let database_url = env::var("DATABASE_URL").context("DATABASE_URL must be set")?;
		// Loopback by default: the hub's data and auth planes are internal seams reached by
		// the BFF (and same-host services). Opt into a wider bind (e.g. 0.0.0.0:50051) only
		// behind network segmentation — see docs/ARCHITECTURE.md.
		let grpc_addr = env::var("GRPC_ADDR")
			.unwrap_or_else(|_| DEFAULT_GRPC_ADDR.to_string())
			.parse()
			.with_context(|| format!("GRPC_ADDR must be a valid socket address, e.g. {DEFAULT_GRPC_ADDR}"))?;
		let auth_grpc_addr = env::var("AUTH_GRPC_ADDR")
			.unwrap_or_else(|_| DEFAULT_AUTH_GRPC_ADDR.to_string())
			.parse()
			.with_context(|| format!("AUTH_GRPC_ADDR must be a valid socket address, e.g. {DEFAULT_AUTH_GRPC_ADDR}"))?;
		let sentry_dsn = env::var("SENTRY_DSN").ok().filter(|s| !s.is_empty());
		let posthog_key = env::var("POSTHOG_KEY").ok().filter(|s| !s.is_empty());
		let posthog_host = env::var("POSTHOG_HOST").ok().filter(|s| !s.is_empty());
		let app_env = env::var("APP_ENV").unwrap_or_else(|_| "development".to_string());
		let tigerbeetle_address = env::var("TIGERBEETLE_ADDRESS").unwrap_or_else(|_| "127.0.0.1:3033".to_string());
		let tigerbeetle_cluster_id = env::var("TIGERBEETLE_CLUSTER_ID")
			.unwrap_or_else(|_| "0".to_string())
			.parse()
			.context("TIGERBEETLE_CLUSTER_ID must be an integer")?;
		let admin_subjects = env::var("ADMIN_SUBJECTS")
			.unwrap_or_default()
			.split(',')
			.map(str::trim)
			.filter(|s| !s.is_empty())
			.map(str::to_owned)
			.collect();
		let signer_grpc_addr = env::var("SIGNER_GRPC_ADDR").unwrap_or_else(|_| "http://127.0.0.1:50053".to_string());
		let db_max_connections = env::var("DB_MAX_CONNECTIONS")
			.ok()
			.map(|v| v.parse().context("DB_MAX_CONNECTIONS must be a positive integer"))
			.transpose()?
			.unwrap_or(10);
		let relay_db_max_connections = env::var("RELAY_DB_MAX_CONNECTIONS")
			.ok()
			.map(|v| v.parse().context("RELAY_DB_MAX_CONNECTIONS must be a positive integer"))
			.transpose()?
			.unwrap_or(3);
		Ok(Self {
			database_url,
			grpc_addr,
			auth_grpc_addr,
			sentry_dsn,
			posthog_key,
			posthog_host,
			app_env,
			tigerbeetle_address,
			tigerbeetle_cluster_id,
			admin_subjects,
			signer_grpc_addr,
			db_max_connections,
			relay_db_max_connections,
		})
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn default_grpc_binds_are_loopback() {
		for raw in [DEFAULT_GRPC_ADDR, DEFAULT_AUTH_GRPC_ADDR] {
			let addr: SocketAddr = raw.parse().expect("default addr parses");
			assert!(addr.ip().is_loopback(), "{raw} must default to loopback, not all interfaces");
		}
	}
}
