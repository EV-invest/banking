//! On-chain deposit watcher — credits user balances from confirmed USDT transfers.
//!
//! A read-mostly background task (sibling to the [`bridge`](super::bridge) consumer): it
//! polls an EVM JSON-RPC for ERC-20 `Transfer` logs of the USDT contract whose `to` is
//! one of our users' **derived** deposit addresses, waits `confirmations` blocks (reorg
//! safety), and records each via [`record_deposit`](crate::application::balance::record_deposit)
//! — idempotent by the on-chain `tx_ref` (`txhash:logindex`), so a re-scan never double-
//! credits. The relay then posts `Dr wallet:<net> / Cr user-claim`; the watcher itself
//! never touches TigerBeetle, so money is still written last, in the relay.
//!
//! Resume is from `deposit_scan_cursor` (per network). On first run it starts at the
//! configured start block, else the current safe head (watch from now). Only blocks at or
//! below `latest − confirmations` are scanned, so shallow reorgs are absorbed; a reorg
//! deeper than `confirmations` is a known, out-of-scope residual (reconciliation territory).
//!
//! Generic over the EVM rail — one instance per chain (BEP20, Polygon), keyed by
//! `config.network`; the raw log value is scaled into canonical base units via
//! [`Usdt::from_onchain`], so a 6-dp rail (Polygon) and an 18-dp rail (BEP20) credit correctly.
//! The `eth_getLogs` `to`-topic filter (an OR over the watched addresses) means the endpoint
//! MUST support `eth_getLogs` — some public nodes don't; point the rail's RPC URL at one that does.

use std::{
	collections::HashMap,
	sync::{
		Arc,
		atomic::{AtomicU64, Ordering},
	},
	time::Duration,
};

use domain::{
	balance::Party,
	money::{Network, TxRef, Usdt},
	users::UserId,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::{
	application::balance::record_deposit,
	config::EvmConfig,
	infrastructure::{custody::ChainCustody, deposits::PgDeposits},
};

/// `keccak256("Transfer(address,address,uint256)")` — the ERC-20 Transfer event topic0.
const TRANSFER_TOPIC: &str = "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";

/// Max retries for a single RPC call (`eth_blockNumber` / `eth_getLogs`) when the
/// error is retryable (rate-limit, timeout). Caps the total backoff within one scan
/// cycle so a stuck endpoint doesn't stall the watcher forever.
const RPC_MAX_RETRIES: u32 = 5;
/// Base backoff in ms before the first retry.
const RPC_BASE_BACKOFF_MS: u64 = 1000;
/// Max backoff between retries — caps the exponential ramp.
const RPC_MAX_BACKOFF_MS: u64 = 30_000;
/// Max poll interval after repeated cycle failures, in seconds — the adaptive ceiling
/// so a down endpoint isn't hammered at the normal cadence.
const CYCLE_MAX_BACKOFF_SECS: u64 = 300;
/// Floor for the adaptive `eth_getLogs` window. Below this the endpoint is refusing the
/// query for a reason other than its width (free tiers also cap how deep into history a
/// getLogs may reach), so narrowing further is pointless and the cycle fails instead.
const MIN_BLOCK_RANGE: u64 = 16;
/// Pause between consecutive `eth_getLogs` chunks while catching up. Without it the
/// catch-up loop fires chunks as fast as the endpoint answers, which is precisely the
/// burst a free-tier rate limit rejects — pacing trades a slower backfill for one that
/// actually completes.
const CHUNK_PACE_MS: u64 = 250;
/// How far the scan may fall behind the safe head before it is called out on its own.
/// A lagging scan means deposits are landing on-chain UNCREDITED, which is worth an alert
/// in a way that an individual throttled RPC call is not.
const LAG_WARN_BLOCKS: u64 = 5_000;

/// The on-chain deposit watcher task. Holds its own pool clone so its polling reads don't
/// compete with request traffic, and the relay `Notify` so a credit dispatches promptly.
pub struct DepositWatcher {
	pool: PgPool,
	deposits: PgDeposits,
	relay: Arc<Notify>,
	http: reqwest::Client,
	config: EvmConfig,
	/// The `eth_getLogs` window actually in use, in blocks. Starts at the configured
	/// `max_block_range` and halves whenever the endpoint rejects the query as too wide, so
	/// the scan settles on the widest window the endpoint accepts. It never widens again:
	/// a free tier's cap is static, and re-probing it would reintroduce the failed call
	/// this narrowing exists to avoid.
	block_range: AtomicU64,
	/// Resolves the rail's treasury + gas-station addresses so an operator's out-of-band
	/// top-up of the hot wallet becomes a ledger fact instead of invisible money. `None`
	/// leaves the watcher user-deposits-only — the pre-existing behaviour, and what an
	/// unwired rail gets.
	custody: Option<Arc<ChainCustody>>,
}

impl DepositWatcher {
	pub fn new(pool: PgPool, relay: Arc<Notify>, config: EvmConfig, custody: Option<Arc<ChainCustody>>) -> Self {
		let http = reqwest::Client::builder()
			.timeout(Duration::from_secs(20))
			.build()
			.expect("reqwest client builds with default config");
		let deposits = PgDeposits::new(pool.clone());
		let block_range = AtomicU64::new(config.max_block_range.max(1));
		Self {
			pool,
			deposits,
			relay,
			http,
			config,
			block_range,
			custody,
		}
	}

	/// The rail's treasury + gas-station addresses, lowercased, or `None` when the rail has
	/// no custody adapter or the signer could not be reached this cycle. A failure must NOT
	/// fail the scan: user deposits are the watcher's primary job and they keep crediting
	/// with or without the treasury view. `ChainCustody` caches each address in a `OnceCell`,
	/// so this is one signer round-trip per process, not per cycle.
	async fn treasury_addresses(&self) -> Option<(String, Option<String>)> {
		let custody = self.custody.as_ref()?;
		match custody.treasury_address().await {
			Ok(treasury) => {
				let gas_station = custody.gas_station_address().await.ok().map(|a| a.to_lowercase());
				Some((treasury.to_lowercase(), gas_station))
			}
			Err(err) => {
				tracing::debug!(network = %self.config.network, "deposit watcher: treasury address unavailable this cycle, watching user deposits only: {err}");
				None
			}
		}
	}

	/// Poll until `shutdown` is cancelled. A failed cycle is logged and retried next poll
	/// from the unchanged cursor — at-least-once, and crediting is idempotent, so nothing is
	/// lost or double-counted.
	///
	/// Consecutive failures ramp the poll interval exponentially (capped at
	/// [`CYCLE_MAX_BACKOFF_SECS`]) so a down or rate-limited endpoint isn't hammered at the
	/// normal cadence. A single successful cycle resets the backoff to `poll_secs`.
	pub async fn run(self, shutdown: CancellationToken) {
		// Report the endpoint the SCAN actually uses, not `rpc_url` — they differ whenever a
		// dedicated logs endpoint is configured, and logging the wrong one sends whoever is
		// debugging a silent watcher to the wrong provider's dashboard.
		info!(network = %self.config.network, rpc = %rpc_host(self.scan_endpoint()), contract = %self.config.usdt_contract, confirmations = self.config.confirmations, "deposit watcher: watching EVM USDT deposits");
		if self.config.logs_rpc_url.is_none() {
			// The fallback is `rpc_url`, chosen for the money-moving paths (nonce, balance,
			// broadcast). The public full nodes that serve those well are exactly the ones that
			// refuse eth_getLogs (-32005), so this combination detects NO deposits at all — a
			// silent failure worth naming loudly at boot rather than discovering from a
			// customer's missing balance.
			warn!(
				network = %self.config.network,
				rpc = %rpc_host(&self.config.rpc_url),
				"deposit watcher: no dedicated eth_getLogs endpoint set (BSC_LOGS_RPC_URL / POLYGON_LOGS_RPC_URL) — falling back to the main RPC, which on the default public nodes REJECTS eth_getLogs; deposits will go undetected until one is configured"
			);
		}
		let mut consecutive_failures: u64 = 0;
		loop {
			let delay = match self.scan_once().await {
				Ok(()) => {
					consecutive_failures = 0;
					self.config.poll_secs
				}
				Err(err) => {
					consecutive_failures += 1;
					let backoff = cycle_backoff_secs(self.config.poll_secs, consecutive_failures);
					warn!(network = %self.config.network, "deposit watcher: scan cycle failed, retrying in {backoff}s (failure #{consecutive_failures}): {err}");
					backoff
				}
			};
			tokio::select! {
				() = shutdown.cancelled() => {
					info!("deposit watcher: shutdown requested — stopping");
					return;
				}
				() = tokio::time::sleep(Duration::from_secs(delay)) => {}
			}
		}
	}

	async fn scan_once(&self) -> Result<(), WatcherError> {
		let network = self.config.network;
		let latest = self.block_number().await?;
		let safe_head = latest.saturating_sub(self.config.confirmations);
		let mut last_scanned = self.cursor(network, safe_head).await?;
		if safe_head <= last_scanned {
			return Ok(());
		}
		// Only `derived` (fundable) addresses can receive a real deposit; a placeholder is
		// never funded. The map is `lower(address) -> owner`, also the `to`-topic filter set.
		let watched = self.watched_addresses(network).await?;
		// The treasury rides along in the SAME filter: USDT arriving there from outside our
		// own wallets is the fund's own capital, and without this it moves real money while
		// leaving no ledger trace at all.
		let (treasury, gas_station) = match self.treasury_addresses().await {
			Some((treasury, gas_station)) => (Some(treasury), gas_station),
			None => (None, None),
		};
		if watched.is_empty() && treasury.is_none() {
			// Nothing fundable yet — fast-forward so we don't re-scan an empty window forever.
			self.set_cursor(network, safe_head).await?;
			return Ok(());
		}
		let topic_addrs: Vec<Value> = watched.keys().chain(treasury.iter()).map(|a| Value::String(pad_topic(a))).collect();

		// A scan that has fallen behind is the condition worth alerting on: those blocks hold
		// deposits nobody has been credited for yet. Reported once per cycle, against the
		// per-cycle backoff, rather than once per throttled RPC call.
		let lag = safe_head - last_scanned;
		if lag > LAG_WARN_BLOCKS {
			warn!(network = %network, lag_blocks = lag, last_scanned, safe_head, "deposit watcher: scan is behind the chain — deposits in the gap are not yet credited");
		}

		while last_scanned < safe_head {
			let from = last_scanned + 1;
			let (logs, to) = self.get_logs(from, safe_head, &topic_addrs).await?;
			for log in &logs {
				let Some(transfer) = decode_transfer(log) else { continue };
				if let Some(&user) = watched.get(&transfer.to) {
					self.credit(Party::User(user), network, &transfer).await?;
				} else if treasury.as_deref() == Some(transfer.to.as_str()) && is_external_source(&transfer.from, &watched, gas_station.as_deref(), treasury.as_deref()) {
					self.credit(Party::Piggybank, network, &transfer).await?;
				}
			}
			// Advance only after the chunk's deposits are recorded. A crash between recording
			// and this update re-scans the chunk; `record_deposit` is idempotent by tx_ref.
			self.set_cursor(network, to).await?;
			last_scanned = to;
			if last_scanned < safe_head {
				tokio::time::sleep(Duration::from_millis(CHUNK_PACE_MS)).await;
			}
		}
		Ok(())
	}

	async fn credit(&self, party: Party, network: Network, transfer: &Transfer) -> Result<(), WatcherError> {
		let amount = Usdt::from_onchain(network, transfer.value).map_err(|e| WatcherError::Decode(e.to_string()))?;
		if amount.is_zero() {
			return Ok(()); // a legal but meaningless zero-value Transfer — not a deposit.
		}
		let tx_ref = TxRef::parse(&transfer.tx_ref()).map_err(|e| WatcherError::Decode(e.to_string()))?;
		let is_capital = matches!(party, Party::Piggybank);
		let newly = record_deposit(&self.deposits, &self.relay, tx_ref, party, network, amount)
			.await
			.map_err(|e| WatcherError::Credit(e.to_string()))?;
		if newly && is_capital {
			// Worth its own line at INFO: this is the fund's own money entering the rail, and
			// the operator who sent it has no other confirmation that it landed in the ledger.
			info!(network = %network, tx = %transfer.tx_hash, from = %transfer.from, "deposit watcher: credited an out-of-band treasury top-up as fund capital");
		} else if newly {
			info!(tx = %transfer.tx_hash, "deposit watcher: credited on-chain USDT deposit");
		}
		Ok(())
	}

	// ── JSON-RPC ────────────────────────────────────────────────────────────────
	/// The endpoint the scan's reads go to: the dedicated logs endpoint when configured,
	/// else the rail's main RPC. Free full nodes increasingly paywall `eth_getLogs`
	/// specifically, so the scan and the money-moving paths can want different providers.
	fn scan_endpoint(&self) -> &str {
		self.config.logs_rpc_url.as_deref().unwrap_or(&self.config.rpc_url)
	}

	async fn block_number(&self) -> Result<u64, WatcherError> {
		let result = self.rpc("eth_blockNumber", json!([])).await?;
		let hex = result.as_str().ok_or_else(|| WatcherError::Rpc("eth_blockNumber: non-string result".into()))?;
		parse_hex_u64(hex).ok_or_else(|| WatcherError::Rpc(format!("eth_blockNumber: unparseable {hex}")))
	}

	/// One chunk of Transfer logs starting at `from`, plus the last block it covers — the
	/// caller advances the cursor to that block rather than assuming the window it asked for.
	///
	/// A "range too wide" rejection is DETERMINISTIC: the identical request fails every time,
	/// so retrying it (as a rate limit is retried) only burns attempts and log lines. The one
	/// response that can succeed is a narrower window, so that is what this does — halving
	/// until the endpoint accepts it or [`MIN_BLOCK_RANGE`] is reached, at which point the
	/// refusal is about something other than width and the cycle fails.
	async fn get_logs(&self, from: u64, safe_head: u64, addresses: &[Value]) -> Result<(Vec<Value>, u64), WatcherError> {
		loop {
			let range = self.block_range.load(Ordering::Relaxed);
			let to = from.saturating_add(range - 1).min(safe_head);
			let params = json!([{
				"fromBlock": format!("0x{from:x}"),
				"toBlock": format!("0x{to:x}"),
				"address": self.config.usdt_contract,
				"topics": [TRANSFER_TOPIC, Value::Null, addresses],
			}]);
			match self.rpc("eth_getLogs", params).await {
				Ok(result) => {
					let logs = result.as_array().cloned().ok_or_else(|| WatcherError::Rpc("eth_getLogs: result is not an array".into()))?;
					return Ok((logs, to));
				}
				Err(err) if matches!(classify(&err.to_string()), RpcAction::Narrow) && range > MIN_BLOCK_RANGE => {
					let narrowed = (range / 2).max(MIN_BLOCK_RANGE);
					self.block_range.store(narrowed, Ordering::Relaxed);
					// Bounded: the window only ever shrinks, so this fires at most
					// log2(max_block_range / MIN_BLOCK_RANGE) times for the process's life.
					warn!(network = %self.config.network, "deposit watcher: endpoint refused a {range}-block eth_getLogs window, narrowing to {narrowed}: {err}");
				}
				Err(err) => return Err(err),
			}
		}
	}

	/// Issue a single JSON-RPC call — no retry, the raw transport. Used by the retry
	/// loop in [`rpc`] and kept thin so the retry counter and backoff live above it.
	async fn rpc_once(&self, method: &str, params: &Value) -> Result<Value, WatcherError> {
		let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
		let response = self
			.http
			.post(self.scan_endpoint())
			.json(&body)
			.send()
			.await
			.map_err(|e| WatcherError::Rpc(format!("{method}: request failed: {e}")))?;
		// A throttling proxy answers 429 (or a 5xx) with an HTML body, not a JSON-RPC error
		// object, so the status has to be inspected before the body is parsed as JSON —
		// otherwise a throttle reads as a permanent "bad json" and the cycle gives up
		// instead of backing off.
		//
		// The body still has to come along. Providers also return a perfectly good JSON-RPC
		// error object WITH a 4xx status (drpc answers an over-wide `eth_getLogs` that way),
		// and that body carries the only signal separating "narrow the window and retry"
		// from "give up". Reporting the bare status discarded it, so every 4xx looked fatal
		// and the adaptive narrowing below never engaged — the Polygon rail sat refused for
		// days behind a `http status 400` that was really a range error.
		let status = response.status();
		if !status.is_success() {
			let body = response.text().await.unwrap_or_default();
			// One line, bounded: this lands in a log and the useful part (the JSON-RPC error
			// object) is at the front. An HTML error page just contributes noise.
			let detail: String = body.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(400).collect();
			return Err(WatcherError::Rpc(format!("{method}: http status {} {detail}", status.as_u16())));
		}
		let response: Value = response.json().await.map_err(|e| WatcherError::Rpc(format!("{method}: bad json: {e}")))?;
		if let Some(err) = response.get("error").filter(|e| !e.is_null()) {
			return Err(WatcherError::Rpc(format!("{method}: rpc error: {err}")));
		}
		response.get("result").cloned().ok_or_else(|| WatcherError::Rpc(format!("{method}: response had no result")))
	}

	/// Call `rpc_once` with exponential-backoff retry for transient errors (rate
	/// limits, timeouts). Non-retryable errors fail immediately; retryable errors
	/// back off 1s → 2s → 4s → 8s → 16s (capped at 30s), up to
	/// [`RPC_MAX_RETRIES`] attempts total. The scan's calls (blockNumber +
	/// getLogs) go to the logs endpoint when one is configured — free full nodes
	/// increasingly paywall eth_getLogs specifically, while the money-moving paths
	/// stay on the (reliable) main rpc_url.
	///
	/// Retries log at DEBUG, not WARN. Being throttled by a public endpoint is the expected
	/// steady state here, and a retry that then succeeds is a non-event; emitting a warning
	/// per attempt turned one throttled call into five alerts. The cycle-level `warn!` in
	/// [`run`] is the signal — it fires once per cycle, after the retries are exhausted, and
	/// rides the cycle backoff instead of the RPC cadence.
	async fn rpc(&self, method: &str, params: Value) -> Result<Value, WatcherError> {
		let mut attempt: u32 = 0;
		loop {
			match self.rpc_once(method, &params).await {
				Ok(val) => return Ok(val),
				Err(e) => {
					let msg = e.to_string();
					if !matches!(classify(&msg), RpcAction::Backoff) || attempt >= RPC_MAX_RETRIES {
						return Err(e);
					}
					attempt += 1;
					let ms = (RPC_BASE_BACKOFF_MS << (attempt - 1)).min(RPC_MAX_BACKOFF_MS);
					tracing::debug!(network = %self.config.network, "deposit watcher: {method} retryable error (attempt {attempt}/{RPC_MAX_RETRIES}), backing off {ms}ms: {msg}");
					tokio::time::sleep(Duration::from_millis(ms)).await;
				}
			}
		}
	}

	// ── cursor + watched addresses (Postgres control plane) ───────────────────────
	/// The last fully-scanned block. On first run, initialize to `start_block − 1` (so the
	/// configured start block is the first scanned) or the current safe head (watch from now).
	async fn cursor(&self, network: Network, default_head: u64) -> Result<u64, WatcherError> {
		let existing: Option<i64> = sqlx::query_scalar("SELECT last_scanned_block FROM deposit_scan_cursor WHERE network = $1")
			.bind(network.as_str())
			.fetch_optional(&self.pool)
			.await
			.map_err(repo)?;
		if let Some(block) = existing {
			return Ok(block.max(0) as u64);
		}
		let init = self.config.start_block.map(|b| b.saturating_sub(1)).unwrap_or(default_head);
		sqlx::query("INSERT INTO deposit_scan_cursor (network, last_scanned_block) VALUES ($1, $2) ON CONFLICT (network) DO NOTHING")
			.bind(network.as_str())
			.bind(init as i64)
			.execute(&self.pool)
			.await
			.map_err(repo)?;
		Ok(init)
	}

	async fn set_cursor(&self, network: Network, block: u64) -> Result<(), WatcherError> {
		sqlx::query("UPDATE deposit_scan_cursor SET last_scanned_block = $2, updated_at = now() WHERE network = $1")
			.bind(network.as_str())
			.bind(block as i64)
			.execute(&self.pool)
			.await
			.map_err(repo)?;
		Ok(())
	}

	async fn watched_addresses(&self, network: Network) -> Result<HashMap<String, UserId>, WatcherError> {
		let rows: Vec<(uuid::Uuid, String)> = sqlx::query_as("SELECT user_id, address FROM user_deposit_addresses WHERE network = $1 AND address_kind = 'derived'")
			.bind(network.as_str())
			.fetch_all(&self.pool)
			.await
			.map_err(repo)?;
		Ok(rows.into_iter().map(|(uid, address)| (address.to_lowercase(), UserId::from_raw(uid))).collect())
	}
}

/// Is USDT arriving at the treasury NEW money, or our own funds being consolidated?
///
/// This is the whole safety argument for crediting the treasury at all. The sweep moves USDT
/// **from a user's derived deposit address to the treasury**, and that dollar is already on the
/// ledger — `wallet:<net>` counts it from the moment the deposit was credited, whichever of our
/// addresses it physically sits on. Crediting it again on arrival would post a second
/// `Dr wallet:<net> / Cr fund` for one dollar and break the global `sum(custody) == sum(claims)`
/// invariant, in the direction that invents capital out of nothing. Sweeps are not an edge case —
/// they are the steady state, so this predicate runs before every capital credit.
///
/// Only a source outside every wallet we control is genuinely new money: not a derived deposit
/// address, not the gas station, and not the treasury paying itself.
fn is_external_source(from: &str, watched: &HashMap<String, UserId>, gas_station: Option<&str>, treasury: Option<&str>) -> bool {
	!watched.contains_key(from) && gas_station != Some(from) && treasury != Some(from)
}

/// One decoded ERC-20 `Transfer` to a watched address.
struct Transfer {
	/// Lowercase `0x…` 20-byte sender — the only thing separating an operator's capital
	/// injection from the sweep consolidating funds we already booked.
	from: String,
	/// Lowercase `0x…` 20-byte recipient address (the matched deposit address).
	to: String,
	/// Transferred value in raw on-chain units, scaled into canonical base units by
	/// [`Usdt::from_onchain`] at the credit edge — 1:1 for BEP20 (18-dp), ×10^12 for
	/// Polygon (6-dp).
	value: u128,
	tx_hash: String,
	log_index: u64,
}

impl Transfer {
	/// The idempotency key for [`record_deposit`]: a single tx can carry several Transfers
	/// (to different users), so the log index disambiguates.
	fn tx_ref(&self) -> String {
		format!("{}:{}", self.tx_hash, self.log_index)
	}
}

/// Decode an `eth_getLogs` Transfer log. `None` if the shape is unexpected or the value
/// exceeds `u128` (an impossible USDT amount we refuse to credit).
fn decode_transfer(log: &Value) -> Option<Transfer> {
	let topics = log.get("topics")?.as_array()?;
	if topics.len() < 3 {
		return None;
	}
	// Defensive: the RPC already filtered on topic0, but verify before crediting.
	if !topics[0].as_str()?.eq_ignore_ascii_case(TRANSFER_TOPIC) {
		return None;
	}
	let from = address_from_topic(topics[1].as_str()?)?;
	let to = address_from_topic(topics[2].as_str()?)?;
	let value = u128_from_word(log.get("data")?.as_str()?)?;
	let tx_hash = log.get("transactionHash")?.as_str()?.to_lowercase();
	let log_index = parse_hex_u64(log.get("logIndex")?.as_str()?)?;
	Some(Transfer { from, to, value, tx_hash, log_index })
}

/// The last 20 bytes of a 32-byte topic word → a lowercase `0x…` address.
fn address_from_topic(topic: &str) -> Option<String> {
	let hex = topic.strip_prefix("0x")?;
	if hex.len() != 64 {
		return None;
	}
	Some(format!("0x{}", &hex[24..]).to_lowercase())
}

/// A 32-byte big-endian uint256 word → `u128`. `None` if it exceeds `u128` (the high 16
/// bytes are non-zero) — refused rather than silently truncated.
fn u128_from_word(word: &str) -> Option<u128> {
	let hex = word.strip_prefix("0x")?;
	if hex.len() != 64 {
		return None;
	}
	let (high, low) = hex.split_at(32);
	if high.bytes().any(|b| b != b'0') {
		return None;
	}
	u128::from_str_radix(low, 16).ok()
}

fn parse_hex_u64(value: &str) -> Option<u64> {
	u64::from_str_radix(value.strip_prefix("0x")?, 16).ok()
}

/// Left-pad a 20-byte `0x` address into a 32-byte topic word for the `to` filter.
fn pad_topic(address_lower: &str) -> String {
	let hex = address_lower.strip_prefix("0x").unwrap_or(address_lower);
	format!("0x{hex:0>64}")
}

/// Host (and port) of the RPC URL, for logging without leaking an API key in the path.
fn rpc_host(url: &str) -> &str {
	url.split("://").nth(1).unwrap_or(url).split('/').next().unwrap_or(url)
}

/// How the caller should react to an RPC error.
#[derive(Debug, PartialEq, Eq)]
enum RpcAction {
	/// Transient (rate limit, timeout, transport, 429/5xx) — the same call may succeed later.
	Backoff,
	/// The endpoint refused the query's block window — only a narrower one can succeed.
	Narrow,
	/// Repeating changes nothing (invalid params, method-not-found, unparseable) — fail now.
	Fail,
}

/// Classify an RPC error message. dRPC's free tier is the endpoint these run against by
/// default, so its codes lead: 15 (rate limit), 30 (timeout), 35 (block window refused).
/// The `Narrow` phrasings cover the other providers' wording for the same condition, since
/// the rail's RPC URL is operator-configured and may point anywhere.
///
/// Splitting `Narrow` out of the retryable set is the point: it used to be lumped in with
/// rate limits and retried five times, and since the identical query is refused every time,
/// those five attempts could only ever fail — five alerts for one unwinnable call.
fn classify(msg: &str) -> RpcAction {
	// Codes are matched against a whitespace-stripped copy: the JSON-RPC error object is
	// reproduced verbatim from the wire, and `{"code": 35}` is as valid as `{"code":35}`.
	let compact: String = msg.chars().filter(|c| !c.is_whitespace()).collect();
	if compact.contains("\"code\":35")
		|| msg.contains("blocks are not supported")
		|| msg.contains("block range")
		|| msg.contains("query returned more than")
		|| msg.contains("exceed maximum block range")
	{
		return RpcAction::Narrow;
	}
	if compact.contains("\"code\":15") || compact.contains("\"code\":30") || msg.contains("request failed") || msg.contains("http status 429") || msg.contains("http status 5") {
		return RpcAction::Backoff;
	}
	RpcAction::Fail
}

/// Poll interval after `failures` consecutive failed cycles: `poll_secs` doubled per
/// failure, capped at [`CYCLE_MAX_BACKOFF_SECS`].
///
/// Saturating throughout. The naive `poll_secs * 2u64.pow(failures - 1)` overflows `u64`
/// around the 60th consecutive failure — reachable in well under a day once the cap pins
/// the cycle at 5 minutes — and an overflow there wraps to a SMALL delay, turning a
/// backed-off watcher back into a hot loop against the endpoint already refusing it.
fn cycle_backoff_secs(poll_secs: u64, failures: u64) -> u64 {
	let shift = u32::try_from(failures.saturating_sub(1)).unwrap_or(u32::MAX).min(63);
	poll_secs.saturating_mul(1u64 << shift).min(CYCLE_MAX_BACKOFF_SECS)
}

#[derive(Debug, thiserror::Error)]
enum WatcherError {
	#[error("rpc: {0}")]
	Rpc(String),
	#[error("decode: {0}")]
	Decode(String),
	#[error("credit: {0}")]
	Credit(String),
	#[error("db: {0}")]
	Db(String),
}

fn repo(err: sqlx::Error) -> WatcherError {
	WatcherError::Db(err.to_string())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn decodes_a_real_transfer_log() {
		// A BEP20 USDT Transfer of 5 USDT (5e18) to 0x024d…63e8.
		let log = json!({
			"topics": [
				TRANSFER_TOPIC,
				"0x0000000000000000000000001111111111111111111111111111111111111111",
				"0x000000000000000000000000024da544a76714a3812096e9ef84d40b2c8863e8"
			],
			"data": "0x0000000000000000000000000000000000000000000000004563918244f40000",
			"transactionHash": "0xABCDEF0000000000000000000000000000000000000000000000000000000001",
			"logIndex": "0x2"
		});
		let t = decode_transfer(&log).expect("valid transfer decodes");
		assert_eq!(t.from, "0x1111111111111111111111111111111111111111");
		assert_eq!(t.to, "0x024da544a76714a3812096e9ef84d40b2c8863e8");
		assert_eq!(t.value, 5_000_000_000_000_000_000); // 5 USDT at 18 dp
		assert_eq!(t.tx_hash, "0xabcdef0000000000000000000000000000000000000000000000000000000001");
		assert_eq!(t.log_index, 2);
		assert_eq!(t.tx_ref(), "0xabcdef0000000000000000000000000000000000000000000000000000000001:2");
	}

	/// The invariant-critical case. A sweep arrives at the treasury exactly like an operator
	/// top-up does — same recipient, same token, same event — and only the sender tells them
	/// apart. Credit a sweep and one dollar is booked twice, inventing fund capital and
	/// breaking `sum(custody) == sum(claims)`.
	#[test]
	fn only_a_source_outside_our_own_wallets_is_new_capital() {
		let deposit_address = "0x024da544a76714a3812096e9ef84d40b2c8863e8";
		let gas_station = "0x7ec1d5446115c39aab004146255ca62f97ca0514";
		let treasury = "0x7303d8dcd615548d8f46b059d1bd31a8b6a3389d";
		let watched: HashMap<String, UserId> = [(deposit_address.to_string(), UserId::from_raw(uuid::Uuid::nil()))].into_iter().collect();

		// The sweep consolidating a user's deposit — already on the ledger, must NOT re-credit.
		assert!(!is_external_source(deposit_address, &watched, Some(gas_station), Some(treasury)));
		// The gas station moving native coin never carries USDT, but it is ours either way.
		assert!(!is_external_source(gas_station, &watched, Some(gas_station), Some(treasury)));
		// The treasury paying itself is not an injection.
		assert!(!is_external_source(treasury, &watched, Some(gas_station), Some(treasury)));
		// An outside wallet — the operator funding the rail. This is the one we credit.
		assert!(is_external_source("0x1347378b1d0eb69d3462e09b3dfa2fe28ebe74ec", &watched, Some(gas_station), Some(treasury)));
	}

	/// A rail whose gas station could not be resolved this cycle must still refuse to count a
	/// sweep as capital — the derived-address set alone carries that guarantee.
	#[test]
	fn an_unresolved_gas_station_does_not_open_the_sweep_hole() {
		let deposit_address = "0x024da544a76714a3812096e9ef84d40b2c8863e8";
		let watched: HashMap<String, UserId> = [(deposit_address.to_string(), UserId::from_raw(uuid::Uuid::nil()))].into_iter().collect();
		assert!(!is_external_source(deposit_address, &watched, None, None));
		assert!(is_external_source("0x1347378b1d0eb69d3462e09b3dfa2fe28ebe74ec", &watched, None, None));
	}

	#[test]
	fn rejects_a_value_exceeding_u128() {
		// High 16 bytes non-zero ⇒ exceeds u128 ⇒ refused, never truncated.
		assert!(u128_from_word("0x0000000000000000000000000000000100000000000000000000000000000000").is_none());
		assert_eq!(u128_from_word("0x0000000000000000000000000000000000000000000000000000000000000001"), Some(1));
	}

	#[test]
	fn pads_address_to_a_32_byte_topic() {
		assert_eq!(
			pad_topic("0x024da544a76714a3812096e9ef84d40b2c8863e8"),
			"0x000000000000000000000000024da544a76714a3812096e9ef84d40b2c8863e8"
		);
	}

	#[test]
	fn a_rate_limit_backs_off_but_a_refused_range_narrows() {
		// The two errors seen in production, verbatim from the dRPC free tier.
		assert_eq!(
			classify(r#"rpc: eth_blockNumber: rpc error: {"code":15,"message":"You reached Public endpoint rate limit, please upgrade to paid plan"}"#),
			RpcAction::Backoff
		);
		assert_eq!(
			classify(r#"rpc: eth_getLogs: rpc error: {"code":35,"message":"ranges over 10000 blocks are not supported on free plan"}"#),
			RpcAction::Narrow
		);
		// A throttling proxy's bare 429/5xx is transient; its parse failure is not the signal.
		assert_eq!(classify("rpc: eth_getLogs: http status 429"), RpcAction::Backoff);
		assert_eq!(classify("rpc: eth_getLogs: http status 503"), RpcAction::Backoff);
		assert_eq!(classify("rpc: eth_getLogs: request failed: connection reset"), RpcAction::Backoff);
		// Other providers' wording for a refused window.
		assert_eq!(classify("rpc: eth_getLogs: rpc error: exceed maximum block range: 5000"), RpcAction::Narrow);
		assert_eq!(classify("rpc: eth_getLogs: rpc error: query returned more than 10000 results"), RpcAction::Narrow);
		// Retrying these changes nothing — fail the cycle instead of burning five attempts.
		assert_eq!(classify(r#"rpc: eth_getLogs: rpc error: {"code":-32601,"message":"method not found"}"#), RpcAction::Fail);
		assert_eq!(classify("rpc: eth_getLogs: bad json: expected value"), RpcAction::Fail);
	}

	#[test]
	fn a_range_error_delivered_with_a_4xx_status_still_narrows() {
		// The regression that left Polygon refused for five days: drpc answers an over-wide
		// eth_getLogs with a JSON-RPC error object AND an HTTP 400. Reporting the bare status
		// dropped the body, so this read as `Fail` and the narrowing never engaged.
		assert_eq!(
			classify(r#"rpc: eth_getLogs: http status 400 {"jsonrpc":"2.0","id":1,"error":{"code":35,"message":"ranges over 10000 blocks are not supported on free plan"}}"#),
			RpcAction::Narrow
		);
		// Same body with the spacing a different serializer produces.
		assert_eq!(
			classify(r#"rpc: eth_getLogs: http status 400 {"error": {"code": 35, "message": "ranges over 10000 blocks are not supported on free plan"}}"#),
			RpcAction::Narrow
		);
		// A rate limit delivered the same way is still transient, not a range problem.
		assert_eq!(
			classify(r#"rpc: eth_blockNumber: http status 429 {"error": {"code": 15, "message": "You reached Public endpoint rate limit"}}"#),
			RpcAction::Backoff
		);
		// A 4xx with nothing actionable in it stays fatal — repeating it changes nothing.
		assert_eq!(classify("rpc: eth_getLogs: http status 400 <html>Bad Request</html>"), RpcAction::Fail);
		assert_eq!(classify("rpc: eth_getLogs: http status 400"), RpcAction::Fail);
	}

	#[test]
	fn narrowing_halves_down_to_the_floor_and_stops() {
		// What `get_logs` walks when an endpoint keeps refusing the window: 500 → 16, then
		// the guard (`range > MIN_BLOCK_RANGE`) stops it and the error surfaces.
		let mut range = 500u64;
		let mut steps = 0;
		while range > MIN_BLOCK_RANGE {
			range = (range / 2).max(MIN_BLOCK_RANGE);
			steps += 1;
			assert!(steps < 64, "narrowing must terminate");
		}
		assert_eq!(range, MIN_BLOCK_RANGE);
		assert_eq!(steps, 5);
	}

	#[test]
	fn cycle_backoff_ramps_then_saturates() {
		assert_eq!(cycle_backoff_secs(12, 0), 12); // no failures yet
		assert_eq!(cycle_backoff_secs(12, 1), 12);
		assert_eq!(cycle_backoff_secs(12, 2), 24);
		assert_eq!(cycle_backoff_secs(12, 3), 48);
		assert_eq!(cycle_backoff_secs(12, 5), 192);
		assert_eq!(cycle_backoff_secs(12, 6), CYCLE_MAX_BACKOFF_SECS); // 384 → capped
		// The regression: a long outage must stay pinned at the ceiling, never wrap around
		// to a short delay that hammers the endpoint that is already refusing us.
		assert_eq!(cycle_backoff_secs(12, 60), CYCLE_MAX_BACKOFF_SECS);
		assert_eq!(cycle_backoff_secs(12, 64), CYCLE_MAX_BACKOFF_SECS);
		assert_eq!(cycle_backoff_secs(12, u64::MAX), CYCLE_MAX_BACKOFF_SECS);
	}

	#[test]
	fn rpc_host_hides_the_path() {
		assert_eq!(rpc_host("https://rpc.ankr.com/bsc/secret-key"), "rpc.ankr.com");
		assert_eq!(rpc_host("https://bsc-dataseed.binance.org/"), "bsc-dataseed.binance.org");
	}
}
