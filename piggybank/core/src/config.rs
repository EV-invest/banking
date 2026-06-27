use std::{env, net::SocketAddr};

use anyhow::Context;
use domain::money::Network;

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
	/// The cross-plane lifecycle bridge consumer's config. `None` (either var unset) leaves
	/// the consumer un-run — unconfigured dev/CI is unaffected, matching the other optional
	/// seams. See [`infrastructure::bridge`](crate::infrastructure::bridge).
	pub bridge: Option<BridgeConfig>,
	/// The on-chain BSC config. `None` (no `BSC_RPC_URL`) leaves every on-chain seam — the
	/// deposit watcher, the withdrawal confirmation watcher, and real custody — un-run, the
	/// same no-op-when-unconfigured stance as the bridge. See
	/// [`infrastructure::deposit_watcher`](crate::infrastructure::deposit_watcher) and
	/// [`infrastructure::withdrawal_watcher`](crate::infrastructure::withdrawal_watcher).
	pub bsc: Option<BscConfig>,
	/// The treasury sweep's config. `Some` only when BSC is configured AND `SWEEP_ENABLED`
	/// is set — it moves user deposit balances on-chain into the treasury, so it is opt-in
	/// (merely configuring deposits/withdrawals does not start it). See
	/// [`infrastructure::sweep`](crate::infrastructure::sweep).
	pub sweep: Option<SweepConfig>,
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
		// The bridge runs only when BOTH the concierge address and the shared token are set —
		// a half-configured bridge (one without the other) is a misconfiguration, not a
		// silent no-op, so it's an error rather than running un-authenticated or address-less.
		let concierge_bridge_addr = env::var("CONCIERGE_BRIDGE_ADDR").ok().filter(|s| !s.is_empty());
		let bridge_service_token = env::var("BRIDGE_SERVICE_TOKEN").ok().filter(|s| !s.is_empty());
		let bridge = match (concierge_bridge_addr, bridge_service_token) {
			(Some(concierge_addr), Some(service_token)) => {
				let poll_secs = env::var("BRIDGE_POLL_SECS")
					.ok()
					.map(|v| v.parse().context("BRIDGE_POLL_SECS must be a positive integer"))
					.transpose()?
					.unwrap_or(5);
				Some(BridgeConfig {
					concierge_addr,
					service_token,
					poll_secs,
				})
			}
			(None, None) => None,
			_ => anyhow::bail!("CONCIERGE_BRIDGE_ADDR and BRIDGE_SERVICE_TOKEN must be set together (the bridge needs both an endpoint and the shared token)"),
		};
		// The on-chain seams run only when BSC_RPC_URL is set (the endpoint must support
		// eth_getLogs for deposit scanning). Everything else has a sensible default —
		// mainnet USDT, 15 confs.
		let bsc = match env::var("BSC_RPC_URL").ok().filter(|s| !s.is_empty()) {
			Some(rpc_url) => Some(BscConfig {
				rpc_url,
				usdt_contract: env::var("BSC_USDT_CONTRACT")
					.ok()
					.filter(|s| !s.is_empty())
					.unwrap_or_else(|| "0x55d398326f99059fF775485246999027B3197955".to_string()),
				confirmations: parse_opt("BSC_CONFIRMATIONS")?.unwrap_or(Network::Bep20.min_confirmations() as u64),
				poll_secs: parse_opt("BSC_POLL_SECS")?.unwrap_or(12),
				start_block: parse_opt("BSC_DEPOSIT_START_BLOCK")?,
				max_block_range: parse_opt("BSC_MAX_BLOCK_RANGE")?.unwrap_or(500),
				chain_id: parse_opt("BSC_CHAIN_ID")?.unwrap_or(56),
				gas_limit: parse_opt("BSC_GAS_LIMIT")?.unwrap_or(100_000),
			}),
			None => None,
		};
		// The sweep moves user funds on-chain, so it runs only when explicitly enabled AND the
		// chain is configured — opt-in, never implied by the deposit/withdraw seams.
		let sweep_enabled = env::var("SWEEP_ENABLED").map(|v| v == "true" || v == "1").unwrap_or(false);
		let sweep = match (&bsc, sweep_enabled) {
			(Some(_), true) => Some(SweepConfig {
				min_usdt: parse_opt("SWEEP_MIN_USDT")?.unwrap_or(1_000_000_000_000_000_000),
				gas_drop_multiple: parse_opt("SWEEP_GAS_DROP_MULTIPLE")?.unwrap_or(3),
				min_gas_drop_wei: parse_opt("SWEEP_MIN_GAS_DROP_WEI")?.unwrap_or(300_000_000_000_000),
				topup_grace_secs: parse_opt("SWEEP_TOPUP_GRACE_SECS")?.unwrap_or(60),
				poll_secs: parse_opt("SWEEP_POLL_SECS")?.unwrap_or(30),
			}),
			(None, true) => anyhow::bail!("SWEEP_ENABLED is set but BSC_RPC_URL is not — the sweep needs the chain configured"),
			_ => None,
		};
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
			bridge,
			bsc,
			sweep,
		})
	}
}

/// Config for the one-way concierge→banking lifecycle bridge consumer.
#[derive(Clone, Debug)]
pub struct BridgeConfig {
	/// The concierge plane's gRPC endpoint serving `UserEvents.PullUserLifecycle`.
	pub concierge_addr: String,
	/// The shared bridge service token (`authorization: Bearer …`), the same value
	/// concierge verifies the pull against.
	pub service_token: String,
	/// Seconds between pulls when the backlog is drained. `BRIDGE_POLL_SECS`; defaults to 5.
	pub poll_secs: u64,
}
/// The on-chain BSC config, shared by the deposit watcher, the withdrawal confirmation
/// watcher, and real custody. Present only when `BSC_RPC_URL` is set; the endpoint MUST
/// support `eth_getLogs` (for deposit scanning).
#[derive(Clone, Debug)]
pub struct BscConfig {
	/// BSC JSON-RPC endpoint (`BSC_RPC_URL`). Switch this (+ `BSC_USDT_CONTRACT`) between
	/// testnet and mainnet — the watcher logic is network-agnostic.
	pub rpc_url: String,
	/// The USDT (BEP20) contract address to watch (`BSC_USDT_CONTRACT`). Defaults to the BSC
	/// mainnet USDT (`0x55d3…7955`, 18-dp); set it to the testnet token for a testnet run.
	pub usdt_contract: String,
	/// Confirmations to wait before crediting a deposit / settling a withdrawal
	/// (`BSC_CONFIRMATIONS`); defaults to the domain's BEP20 value (15) — reorg safety.
	pub confirmations: u64,
	/// Seconds between polls, for both the deposit scan and the withdrawal-receipt check
	/// (`BSC_POLL_SECS`); defaults to 12.
	pub poll_secs: u64,
	/// First block to scan on a fresh cursor (`BSC_DEPOSIT_START_BLOCK`). `None` ⇒ start at
	/// the current safe head (watch from now), ignoring pre-existing on-chain history.
	pub start_block: Option<u64>,
	/// Max blocks per `eth_getLogs` call (`BSC_MAX_BLOCK_RANGE`); defaults to 500 to stay
	/// within common provider range limits.
	pub max_block_range: u64,
	/// Chain id for signing withdrawals (`BSC_CHAIN_ID`); 56 = BSC mainnet, 97 = testnet.
	pub chain_id: u64,
	/// Gas limit for an ERC-20 transfer withdrawal (`BSC_GAS_LIMIT`); defaults to 100_000 (a
	/// USDT transfer is ~50–65k — the headroom is safe, and unused gas is refunded).
	pub gas_limit: u64,
}
/// Treasury-sweep economics. `Some` only when BSC is configured AND `SWEEP_ENABLED` is set
/// (it moves user funds on-chain — opt-in). The chain params (rpc, USDT, chain id, the
/// transfer gas limit) come from [`BscConfig`]; these knobs tune *when* and *how much*.
#[derive(Clone, Debug)]
pub struct SweepConfig {
	/// Minimum USDT (18-dp base units) on a deposit address worth sweeping (`SWEEP_MIN_USDT`);
	/// defaults to 1 USDT — below this the gas isn't worth it.
	pub min_usdt: u128,
	/// A BNB top-up sends `max(needed_gas × this, min_gas_drop_wei)` (`SWEEP_GAS_DROP_MULTIPLE`);
	/// defaults to 3, so one top-up covers several future sweeps.
	pub gas_drop_multiple: u128,
	/// Floor for a BNB top-up, in wei (`SWEEP_MIN_GAS_DROP_WEI`); defaults to 3e14 (0.0003 BNB).
	pub min_gas_drop_wei: u128,
	/// Don't re-top-up the same address within this many seconds (`SWEEP_TOPUP_GRACE_SECS`);
	/// defaults to 60 — long enough for a top-up to confirm before we'd consider another.
	pub topup_grace_secs: u64,
	/// Seconds between sweep cycles (`SWEEP_POLL_SECS`); defaults to 30.
	pub poll_secs: u64,
}
/// Parse an optional env var that, when present and non-empty, must be a valid `T`.
fn parse_opt<T: std::str::FromStr>(key: &str) -> anyhow::Result<Option<T>>
where
	T::Err: std::fmt::Display, {
	match env::var(key).ok().filter(|s| !s.is_empty()) {
		Some(raw) => raw.parse::<T>().map(Some).map_err(|e| anyhow::anyhow!("{key} must be a valid value: {e}")),
		None => Ok(None),
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
