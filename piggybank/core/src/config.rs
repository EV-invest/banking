use std::{env, net::SocketAddr};

use domain::money::Network;
use smart_default::SmartDefault;
use v_utils::macros as v_macros;

/// Default gRPC binds: loopback, so the hub's internal data/auth seams are not exposed on
/// every interface. A wider bind is an explicit opt-in that requires network segmentation
/// (see `docs/ARCHITECTURE.md`).
const DEFAULT_GRPC_ADDR: &str = "127.0.0.1:50051";
const DEFAULT_AUTH_GRPC_ADDR: &str = "127.0.0.1:50052";

/// Application configuration (LiveSettings). Prod runs `--config` on the baked
/// `deploy/piggybank.nix` result — `{ env = "VAR" }` refs there assert the var's
/// presence at startup. Dev runs config-less from the flake-exported env
/// (`#[settings(use_env = true)]` aliases each field to its SHOUTY name).
///
/// The on-chain rails (BSC/TRON/TON + sweeps) live in [`Rails`], on their
/// original env-based conditional construction; production boot-asserts the
/// rail set in `main.rs` (at least one rail, TON mainnet keyed).
#[derive(Clone, Debug, v_macros::LiveSettings, v_macros::MyConfigPrimitives, v_macros::Settings, SmartDefault)]
#[settings(use_env = true)]
pub struct AppConfig {
	pub database_url: String,
	/// Core tonic gRPC listener address (the hub's data-plane services). Loopback by
	/// default: an internal seam reached by the BFF (and same-host services). Opt into
	/// a wider bind (e.g. 0.0.0.0:50051) only behind network segmentation.
	#[default(DEFAULT_GRPC_ADDR.parse().unwrap())]
	pub grpc_addr: SocketAddr,
	/// Auth service gRPC listener address (token issuance routes for clients).
	#[default(DEFAULT_AUTH_GRPC_ADDR.parse().unwrap())]
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
	/// TigerBeetle cluster id (a u128, so it rides as a string through every config
	/// source — the settings flags layer has no u128 lane). `"0"` for single-node dev.
	pub tigerbeetle_cluster_id: String,
	/// Break-glass role override: subjects treated as `Owner` by the RBAC gate even
	/// with no mirrored role — the bootstrap path before the identity plane grants
	/// roles. Comma-separated (empty ⇒ no override).
	#[serde(default)]
	pub admin_subjects: Vec<String>,
	/// Endpoint of the separate-process signer (the key vault), for deposit-address
	/// provisioning over the `signer.v1` gRPC seam. The hub connects lazily, so this
	/// only needs to resolve by the time the first address is provisioned.
	pub signer_grpc_addr: String,
	/// Max connections for the request-serving Postgres pool (the core gRPC handlers).
	#[default(10)]
	pub db_max_connections: u32,
	/// Max connections for the outbox relay's own dedicated Postgres pool, so request
	/// traffic and money dispatch can't exhaust each other. A small pool suffices since
	/// the relay is a single-worker drainer (one drain connection + the lock-holding
	/// connection).
	#[default(3)]
	pub relay_db_max_connections: u32,
	/// The concierge plane's gRPC endpoint serving `UserEvents.PullUserLifecycle` —
	/// the cross-plane lifecycle bridge the consumer pulls from.
	pub concierge_bridge_addr: String,
	/// The shared bridge service token (`authorization: Bearer …`), the same value
	/// concierge verifies the pull against.
	pub bridge_service_token: String,
	/// Seconds between bridge pulls when the backlog is drained.
	#[default(5)]
	pub bridge_poll_secs: u64,
}

/// The on-chain rail configs, kept on their original env-based conditional
/// construction (a rail runs only when its endpoint var is set), so they are
/// NOT part of the boot-asserted [`AppConfig`] — production instead asserts
/// the rail set in `main.rs` (a rail-less prod boot is refused).
#[derive(Clone, Debug)]
pub struct Rails {
	/// The on-chain BSC config. `None` (no `BSC_RPC_URL`) leaves every on-chain seam — the
	/// deposit watcher, the withdrawal confirmation watcher, and real custody — un-run. See
	/// [`infrastructure::deposit_watcher`](crate::infrastructure::deposit_watcher) and
	/// [`infrastructure::withdrawal_watcher`](crate::infrastructure::withdrawal_watcher).
	pub bsc: Option<BscConfig>,
	/// The treasury sweep's config. `Some` only when BSC is configured AND `SWEEP_ENABLED`
	/// is set — it moves user deposit balances on-chain into the treasury, so it is opt-in
	/// (merely configuring deposits/withdrawals does not start it). See
	/// [`infrastructure::sweep`](crate::infrastructure::sweep).
	pub sweep: Option<SweepConfig>,
	/// The on-chain Tron (TRC20) config. `None` (no `TRON_RPC_URL`) leaves every Tron seam — the
	/// deposit watcher, the withdrawal confirmation watcher, and real custody — un-run, the same
	/// no-op-when-unconfigured stance as BSC. See [`infrastructure::tron_rpc`](crate::infrastructure::tron_rpc).
	pub tron: Option<TronConfig>,
	/// The Tron treasury sweep's config. `Some` only when Tron is configured AND `SWEEP_ENABLED`
	/// is set — the same opt-in gate as the BEP20 [`sweep`](Self::sweep).
	pub tron_sweep: Option<TronSweepConfig>,
	/// The on-chain TON config. `None` (no `TON_API_URL`) leaves every TON seam — the
	/// jetton deposit watcher, the withdrawal confirmation watcher, and real custody —
	/// un-run, the same no-op-when-unconfigured stance as BSC. Point `TON_API_URL` at
	/// `https://testnet.toncenter.com/api/v3` (`TON_IS_TESTNET=true`) for bring-up.
	pub ton: Option<TonConfig>,
	/// The TON treasury sweep's config. `Some` only when TON is configured AND
	/// `SWEEP_ENABLED` is set (the same opt-in gate as the BSC sweep).
	pub ton_sweep: Option<TonSweepConfig>,
}
impl Rails {
	pub fn from_env() -> color_eyre::Result<Self> {
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
				logs_rpc_url: env::var("BSC_LOGS_RPC_URL").ok().filter(|s| !s.is_empty()),
				chain_id: parse_opt("BSC_CHAIN_ID")?.unwrap_or(56),
				gas_limit: parse_opt("BSC_GAS_LIMIT")?.unwrap_or(100_000),
			}),
			None => None,
		};
		// TRC20 is FROZEN pending an energy-staking gas model — see EV-invest/banking#31. Burning
		// TRX per transfer (~15-30 TRX ≈ $2-5) loses money against the flat withdrawal fee, so the
		// whole rail is off: forcing `tron = None` here (regardless of TRON_RPC_URL) disables the
		// deposit/withdrawal watchers, the sweep, custody, the wallet's TRC20 deposit/withdraw UI,
		// and `configured_networks()` in one place. Re-enable = drop this `TRC20_FROZEN` gate once
		// #31 lands. The TRON seams below stay compiled (a byte-for-byte mirror of BEP20/TON), just
		// never constructed.
		const TRC20_FROZEN: bool = true;
		// Same no-op-when-unconfigured stance as BSC, gated on TRON_RPC_URL (TronGrid REST serves
		// both `/wallet/*` and the indexed `/v1/accounts/*`) — but short-circuited while frozen.
		let tron = match env::var("TRON_RPC_URL").ok().filter(|s| !s.is_empty()) {
			Some(rpc_url) if !TRC20_FROZEN => Some(TronConfig {
				rpc_url,
				usdt_contract: env::var("TRON_USDT_CONTRACT")
					.ok()
					.filter(|s| !s.is_empty())
					.unwrap_or_else(|| "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t".to_string()),
				api_key: env::var("TRON_API_KEY").ok().filter(|s| !s.is_empty()),
				poll_secs: parse_opt("TRON_POLL_SECS")?.unwrap_or(12),
				start_timestamp: parse_opt("TRON_DEPOSIT_START_TS")?,
				fee_limit: parse_opt("TRON_FEE_LIMIT")?.unwrap_or(100_000_000),
				expiration_secs: parse_opt("TRON_EXPIRATION_SECS")?.unwrap_or(60),
				max_transfers_per_scan: parse_opt("TRON_MAX_TRANSFERS")?.unwrap_or(200),
			}),
			_ => None, // unconfigured OR frozen (TRC20_FROZEN) ⇒ rail off
		};
		// The on-chain TON seams run only when TON_API_URL is set (a toncenter v3 base URL).
		// Everything else defaults — mainnet USDT jetton master, 6s poll.
		let ton = match env::var("TON_API_URL").ok().filter(|s| !s.is_empty()) {
			Some(api_url) => {
				// Default the testnet flag from the URL (the testnet toncenter lives on a `testnet.`
				// host), so it moves together with the endpoint. Fail LOUD on EITHER dangerous
				// mismatch against the standard toncenter hosts, because the flag drives the
				// user-facing deposit-address tag and a wrong tag points the user at the wrong
				// network (a stranded, uncredited deposit):
				//   - a testnet URL with the flag off ⇒ testnet rail minting MAINNET-tagged addresses;
				//   - the mainnet host with the flag on ⇒ mainnet rail minting TESTNET-tagged addresses.
				// A CUSTOM endpoint (neither the mainnet nor the testnet toncenter host) is trusted to
				// the explicit TON_IS_TESTNET — that is the custom-testnet-proxy escape hatch.
				let url_is_testnet = api_url.contains("testnet");
				let url_is_mainnet_host = api_url.contains("toncenter.com") && !url_is_testnet;
				let is_testnet = env::var("TON_IS_TESTNET")
					.ok()
					.filter(|s| !s.is_empty())
					.map(|v| v == "true" || v == "1")
					.unwrap_or(url_is_testnet);
				if url_is_testnet && !is_testnet {
					color_eyre::eyre::bail!(
						"TON_API_URL ({api_url}) is a testnet endpoint but TON_IS_TESTNET is not true — user-facing TON deposit addresses would carry the mainnet tag for a testnet rail; set TON_IS_TESTNET=true"
					);
				}
				if url_is_mainnet_host && is_testnet {
					color_eyre::eyre::bail!(
						"TON_API_URL ({api_url}) is the mainnet toncenter host but TON_IS_TESTNET is true — user-facing TON deposit addresses would carry the testnet tag for a mainnet rail; unset TON_IS_TESTNET or point TON_API_URL at a testnet/custom endpoint"
					);
				}
				Some(TonConfig {
					api_url,
					api_key: env::var("TON_API_KEY").ok().filter(|s| !s.is_empty()),
					usdt_master: env::var("TON_USDT_MASTER")
						.ok()
						.filter(|s| !s.is_empty())
						.unwrap_or_else(|| "EQCxE6mUtQJKFnGfaROTKOt1lZbDiiX1kCixRv7Nw2Id_sDs".to_string()),
					poll_secs: parse_opt("TON_POLL_SECS")?.unwrap_or(6),
					start_cursor: parse_opt("TON_DEPOSIT_START_UTIME")?,
					is_testnet,
					wallet_version: env::var("TON_WALLET_VERSION").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "v4r2".to_string()),
					forward_ton_amount: parse_opt("TON_FORWARD_TON_AMOUNT")?.unwrap_or(50_000_000),
					msg_value: parse_opt("TON_MSG_VALUE")?.unwrap_or(100_000_000),
				})
			}
			None => None,
		};
		// The sweep moves user funds on-chain, so it runs only when explicitly enabled AND a chain
		// is configured — opt-in, never implied by the deposit/withdraw seams. `SWEEP_ENABLED` is
		// the global default; a per-rail `<RAIL>_SWEEP_ENABLED` overrides it so a freshly-configured
		// rail can run deposits+withdrawals WITHOUT arming its fund-moving sweep before its treasury
		// and gas station are funded — the sequenced bring-up BEP20 used. An unset per-rail var
		// inherits the global; an unconfigured rail simply has no sweep.
		let sweep_enabled = bool_env("SWEEP_ENABLED", false);
		let bsc_sweep_enabled = bool_env("BSC_SWEEP_ENABLED", sweep_enabled);
		let tron_sweep_enabled = bool_env("TRON_SWEEP_ENABLED", sweep_enabled);
		let ton_sweep_enabled = bool_env("TON_SWEEP_ENABLED", sweep_enabled);
		let sweep = if bsc_sweep_enabled && bsc.is_some() {
			Some(SweepConfig {
				min_usdt: parse_opt("SWEEP_MIN_USDT")?.unwrap_or(1_000_000_000_000_000_000),
				gas_drop_multiple: parse_opt("SWEEP_GAS_DROP_MULTIPLE")?.unwrap_or(3),
				min_gas_drop_wei: parse_opt("SWEEP_MIN_GAS_DROP_WEI")?.unwrap_or(300_000_000_000_000),
				topup_grace_secs: parse_opt("SWEEP_TOPUP_GRACE_SECS")?.unwrap_or(60),
				poll_secs: parse_opt("SWEEP_POLL_SECS")?.unwrap_or(30),
			})
		} else {
			None
		};
		let tron_sweep = if tron_sweep_enabled && tron.is_some() {
			Some(TronSweepConfig {
				min_usdt: parse_opt("TRON_SWEEP_MIN_USDT")?.unwrap_or(1_000_000),
				min_trx_drop_sun: parse_opt("TRON_SWEEP_MIN_TRX_DROP_SUN")?.unwrap_or(30_000_000),
				topup_grace_secs: parse_opt("TRON_SWEEP_TOPUP_GRACE_SECS")?.unwrap_or(60),
				poll_secs: parse_opt("TRON_SWEEP_POLL_SECS")?.unwrap_or(30),
			})
		} else {
			None
		};
		let ton_sweep = if ton_sweep_enabled && ton.is_some() {
			Some(TonSweepConfig {
				min_usdt: parse_opt("TON_SWEEP_MIN_USDT")?.unwrap_or(1_000_000),
				gas_topup_nano: parse_opt("TON_SWEEP_GAS_TOPUP_NANO")?.unwrap_or(100_000_000),
				topup_grace_secs: parse_opt("TON_SWEEP_TOPUP_GRACE_SECS")?.unwrap_or(60),
				poll_secs: parse_opt("TON_SWEEP_POLL_SECS")?.unwrap_or(30),
			})
		} else {
			None
		};
		Ok(Self {
			bsc,
			sweep,
			tron,
			tron_sweep,
			ton,
			ton_sweep,
		})
	}

	/// The rails whose chain config is present — exactly the rails whose deposit watcher
	/// runs (the same `BSC_RPC_URL`/`TRON_RPC_URL`/`TON_API_URL` gates as above). Only
	/// configured rails are provisioned/served by the wallet surface: an address minted
	/// for a rail no watcher scans would strand whatever is sent to it.
	pub fn configured_networks(&self) -> Vec<Network> {
		[
			self.bsc.as_ref().map(|_| Network::Bep20),
			self.tron.as_ref().map(|_| Network::Trc20),
			self.ton.as_ref().map(|_| Network::Ton),
		]
		.into_iter()
		.flatten()
		.collect()
	}
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
	/// Dedicated endpoint for the deposit scan (`BSC_LOGS_RPC_URL`), when the main
	/// `rpc_url` paywalls/throttles `eth_getLogs` (dataseed rejects it outright as of
	/// 2026-07) but is otherwise a fine full node. `None` ⇒ the scan uses `rpc_url`.
	pub logs_rpc_url: Option<String>,
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
/// The on-chain Tron (TRC20) config, shared by the deposit watcher, the withdrawal confirmation
/// watcher, the sweep, and real custody. Present only when `TRON_RPC_URL` is set.
#[derive(Clone, Debug)]
pub struct TronConfig {
	/// TronGrid base URL (`TRON_RPC_URL`). Switch between mainnet (`https://api.trongrid.io`) and
	/// the Nile testnet (`https://nile.trongrid.io`) here — the watcher logic is network-agnostic.
	pub rpc_url: String,
	/// The USDT (TRC20) contract, Base58Check `T…` (`TRON_USDT_CONTRACT`). Defaults to mainnet USDT
	/// (`TR7N…Lj6t`, 6-dp); set it to the Nile faucet token for a testnet run.
	pub usdt_contract: String,
	/// Optional TronGrid API key (`TRON_API_KEY`, sent as `TRON-PRO-API-KEY`). Lifts rate limits on
	/// mainnet; the testnets don't need one.
	pub api_key: Option<String>,
	/// Seconds between deposit/withdrawal-confirmation polls (`TRON_POLL_SECS`); defaults to 12.
	pub poll_secs: u64,
	/// First `block_timestamp` (unix ms) to scan on a fresh cursor (`TRON_DEPOSIT_START_TS`). `None`
	/// starts from the current head (watch from now), ignoring pre-existing on-chain history.
	pub start_timestamp: Option<i64>,
	/// `fee_limit` (SUN) cap for a TRC20 transfer (`TRON_FEE_LIMIT`); defaults to 100_000_000
	/// (100 TRX) — comfortably above a USDT transfer's energy burn, low enough to bound a misfire.
	pub fee_limit: i64,
	/// Seconds a signed transaction stays valid past the head block (`TRON_EXPIRATION_SECS`);
	/// defaults to 60. After this a not-yet-mined tx is provably dead and can be safely re-signed.
	pub expiration_secs: i64,
	/// Max TRC20 transfers fetched per address per deposit scan (`TRON_MAX_TRANSFERS`); defaults to
	/// 200 (the indexed-history page size), the analogue of BSC's `max_block_range`.
	pub max_transfers_per_scan: u32,
}
/// Tron treasury-sweep economics. `Some` only when Tron is configured AND `SWEEP_ENABLED` is set.
/// The chain params (rpc, USDT, fee limit) come from [`TronConfig`]; these tune *when*/*how much*.
#[derive(Clone, Debug)]
pub struct TronSweepConfig {
	/// Minimum USDT (6-dp base units) on a deposit address worth sweeping (`TRON_SWEEP_MIN_USDT`);
	/// defaults to 1_000_000 (1 USDT) — below this the fee isn't worth it.
	pub min_usdt: u128,
	/// TRX (SUN) to drop on an address short of fees (`TRON_SWEEP_MIN_TRX_DROP_SUN`); defaults to
	/// 30_000_000 (30 TRX) — covers a worst-case fresh-recipient USDT-transfer energy burn.
	pub min_trx_drop_sun: u128,
	/// Don't re-top-up the same address within this many seconds (`TRON_SWEEP_TOPUP_GRACE_SECS`);
	/// defaults to 60.
	pub topup_grace_secs: u64,
	/// Seconds between sweep cycles (`TRON_SWEEP_POLL_SECS`); defaults to 30.
	pub poll_secs: u64,
}
/// The on-chain TON config, shared by the jetton deposit watcher, the withdrawal
/// confirmation watcher, and real custody. Present only when `TON_API_URL` is set (a
/// toncenter v3 base URL). Switch it (+ `TON_USDT_MASTER`, `TON_IS_TESTNET`) between
/// mainnet and testnet — the watcher/custody logic is network-agnostic.
#[derive(Clone, Debug)]
pub struct TonConfig {
	/// toncenter v3 base URL (`TON_API_URL`), e.g. `https://toncenter.com/api/v3` or
	/// `https://testnet.toncenter.com/api/v3`.
	pub api_url: String,
	/// toncenter API key (`TON_API_KEY`), sent as `X-Api-Key`. `None` uses the anonymous
	/// (rate-limited) tier — fine for testnet bring-up.
	pub api_key: Option<String>,
	/// The USDT jetton master address (`TON_USDT_MASTER`). Defaults to mainnet USDT; set it
	/// to the testnet master (`kQD0GKBM…`) for a testnet run.
	pub usdt_master: String,
	/// Seconds between polls, for both the jetton-deposit scan and the withdrawal-seqno
	/// check (`TON_POLL_SECS`); defaults to 6 (TON finality is ~5s).
	pub poll_secs: u64,
	/// Start watermark for a fresh deposit cursor (`TON_DEPOSIT_START_UTIME`, unix seconds). `None` ⇒ start at
	/// the current time (watch from now), ignoring pre-existing on-chain history. NOTE: the
	/// watcher tracks the cursor as a unix-time watermark (a globally-comparable value), not
	/// a per-account logical time — see [`infrastructure::ton_deposit_watcher`].
	pub start_cursor: Option<u64>,
	/// Whether the rail is on TON testnet. Defaults from `TON_API_URL` (a `testnet.` host) and
	/// can be set explicitly with `TON_IS_TESTNET`; a testnet URL with the flag off is rejected
	/// (see [`AppConfig::from_env`]). Selects the user-facing address tag and is carried into the
	/// signer so signing stays consistent with the addresses the operator funds.
	pub is_testnet: bool,
	/// The wallet contract version (`TON_WALLET_VERSION`); only `v4r2` is supported today.
	pub wallet_version: String,
	/// Nanotons forwarded with a jetton transfer to deploy the recipient's jetton wallet (if
	/// absent) and trigger its notification (`TON_FORWARD_TON_AMOUNT`); defaults to 0.05 TON.
	pub forward_ton_amount: u64,
	/// Nanotons attached to the jetton-wallet internal message (the gas budget; excess
	/// returns to the response destination) (`TON_MSG_VALUE`); defaults to 0.1 TON.
	pub msg_value: u64,
}

/// TON treasury-sweep economics. `Some` only when TON is configured AND `SWEEP_ENABLED`
/// is set (it moves user funds on-chain — opt-in). The chain params (api, master, fee
/// budgets) come from [`TonConfig`]; these knobs tune *when* and *how much*.
#[derive(Clone, Debug)]
pub struct TonSweepConfig {
	/// Minimum USDT (6-dp base units) on a user's jetton wallet worth sweeping
	/// (`TON_SWEEP_MIN_USDT`); defaults to 1 USDT — below this the gas isn't worth it.
	pub min_usdt: u128,
	/// Nanotons the gas station tops a user wallet up with before sweeping its USDT
	/// (`TON_SWEEP_GAS_TOPUP_NANO`); defaults to 0.1 TON (covers the deploy + jetton send).
	pub gas_topup_nano: u64,
	/// Don't re-top-up the same address within this many seconds (`TON_SWEEP_TOPUP_GRACE_SECS`);
	/// defaults to 60 — long enough for a top-up to confirm before another is considered.
	pub topup_grace_secs: u64,
	/// Seconds between sweep cycles (`TON_SWEEP_POLL_SECS`); defaults to 30.
	pub poll_secs: u64,
}

/// A boolean env var: `true`/`1` ⇒ true, anything else ⇒ false, unset/empty ⇒ `default`.
fn bool_env(key: &str, default: bool) -> bool {
	match env::var(key).ok().filter(|s| !s.is_empty()) {
		Some(v) => v == "true" || v == "1",
		None => default,
	}
}

/// Parse an optional env var that, when present and non-empty, must be a valid `T`.
fn parse_opt<T: std::str::FromStr>(key: &str) -> color_eyre::Result<Option<T>>
where
	T::Err: std::fmt::Display, {
	match env::var(key).ok().filter(|s| !s.is_empty()) {
		Some(raw) => raw.parse::<T>().map(Some).map_err(|e| color_eyre::eyre::eyre!("{key} must be a valid value: {e}")),
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

	/// A minimal rails config with the given chain Options set — only the fields
	/// `configured_networks` reads matter; the rest are inert placeholders.
	fn config(bsc: bool, tron: bool, ton: bool) -> Rails {
		Rails {
			bsc: bsc.then(|| BscConfig {
				rpc_url: String::new(),
				usdt_contract: String::new(),
				confirmations: 1,
				poll_secs: 1,
				start_block: None,
				max_block_range: 1,
				logs_rpc_url: None,
				chain_id: 1,
				gas_limit: 1,
			}),
			sweep: None,
			tron: tron.then(|| TronConfig {
				rpc_url: String::new(),
				usdt_contract: String::new(),
				api_key: None,
				poll_secs: 1,
				start_timestamp: None,
				fee_limit: 1,
				expiration_secs: 1,
				max_transfers_per_scan: 1,
			}),
			tron_sweep: None,
			ton: ton.then(|| TonConfig {
				api_url: String::new(),
				api_key: None,
				usdt_master: String::new(),
				poll_secs: 1,
				start_cursor: None,
				is_testnet: false,
				wallet_version: String::new(),
				forward_ton_amount: 1,
				msg_value: 1,
			}),
			ton_sweep: None,
		}
	}

	#[test]
	fn configured_networks_mirror_the_chain_config_options() {
		assert!(config(false, false, false).configured_networks().is_empty());
		assert_eq!(config(true, false, false).configured_networks(), [Network::Bep20]);
		assert_eq!(config(false, true, false).configured_networks(), [Network::Trc20]);
		assert_eq!(config(false, false, true).configured_networks(), [Network::Ton]);
		assert_eq!(config(true, true, true).configured_networks(), [Network::Bep20, Network::Trc20, Network::Ton]);
	}
}
