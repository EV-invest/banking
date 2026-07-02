//! On-chain deposit watcher — credits user balances from confirmed USDT transfers.
//!
//! A read-mostly background task (sibling to the [`bridge`](super::bridge) consumer): it
//! polls the BSC JSON-RPC for ERC-20 `Transfer` logs of the USDT contract whose `to` is
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
//! Scope today: **BEP20 only**. The `eth_getLogs` `to`-topic filter (an OR over the watched
//! addresses) means the endpoint MUST support `eth_getLogs` — some public BSC nodes don't;
//! point `BSC_RPC_URL` at one that does.

use std::{collections::HashMap, sync::Arc, time::Duration};

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

use crate::{application::balance::record_deposit, config::BscConfig, infrastructure::deposits::PgDeposits};

/// `keccak256("Transfer(address,address,uint256)")` — the ERC-20 Transfer event topic0.
const TRANSFER_TOPIC: &str = "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";

/// The on-chain deposit watcher task. Holds its own pool clone so its polling reads don't
/// compete with request traffic, and the relay `Notify` so a credit dispatches promptly.
pub struct DepositWatcher {
	pool: PgPool,
	deposits: PgDeposits,
	relay: Arc<Notify>,
	http: reqwest::Client,
	config: BscConfig,
}

impl DepositWatcher {
	pub fn new(pool: PgPool, relay: Arc<Notify>, config: BscConfig) -> Self {
		let http = reqwest::Client::builder()
			.timeout(Duration::from_secs(20))
			.build()
			.expect("reqwest client builds with default config");
		let deposits = PgDeposits::new(pool.clone());
		Self {
			pool,
			deposits,
			relay,
			http,
			config,
		}
	}

	/// Poll until `shutdown` is cancelled. A failed cycle is logged and retried next poll
	/// from the unchanged cursor — at-least-once, and crediting is idempotent, so nothing is
	/// lost or double-counted.
	pub async fn run(self, shutdown: CancellationToken) {
		info!(rpc = %rpc_host(&self.config.rpc_url), contract = %self.config.usdt_contract, confirmations = self.config.confirmations, "deposit watcher: watching BEP20 USDT deposits");
		loop {
			if let Err(err) = self.scan_once().await {
				warn!("deposit watcher: scan cycle failed, retrying next poll: {err}");
			}
			tokio::select! {
				() = shutdown.cancelled() => {
					info!("deposit watcher: shutdown requested — stopping");
					return;
				}
				() = tokio::time::sleep(Duration::from_secs(self.config.poll_secs)) => {}
			}
		}
	}

	async fn scan_once(&self) -> Result<(), WatcherError> {
		let network = Network::Bep20;
		let latest = self.block_number().await?;
		let safe_head = latest.saturating_sub(self.config.confirmations);
		let mut last_scanned = self.cursor(network, safe_head).await?;
		if safe_head <= last_scanned {
			return Ok(());
		}
		// Only `derived` (fundable) addresses can receive a real deposit; a placeholder is
		// never funded. The map is `lower(address) -> owner`, also the `to`-topic filter set.
		let watched = self.watched_addresses(network).await?;
		if watched.is_empty() {
			// Nothing fundable yet — fast-forward so we don't re-scan an empty window forever.
			self.set_cursor(network, safe_head).await?;
			return Ok(());
		}
		let topic_addrs: Vec<Value> = watched.keys().map(|a| Value::String(pad_topic(a))).collect();

		while last_scanned < safe_head {
			let from = last_scanned + 1;
			let to = (from + self.config.max_block_range - 1).min(safe_head);
			let logs = self.get_logs(from, to, &topic_addrs).await?;
			for log in &logs {
				let Some(transfer) = decode_transfer(log) else { continue };
				let Some(&user) = watched.get(&transfer.to) else { continue };
				self.credit(user, network, &transfer).await?;
			}
			// Advance only after the chunk's deposits are recorded. A crash between recording
			// and this update re-scans the chunk; `record_deposit` is idempotent by tx_ref.
			self.set_cursor(network, to).await?;
			last_scanned = to;
		}
		Ok(())
	}

	async fn credit(&self, user: UserId, network: Network, transfer: &Transfer) -> Result<(), WatcherError> {
		let amount = Usdt::from_base_units(transfer.value);
		if amount.is_zero() {
			return Ok(()); // a legal but meaningless zero-value Transfer — not a deposit.
		}
		let tx_ref = TxRef::parse(&transfer.tx_ref()).map_err(|e| WatcherError::Decode(e.to_string()))?;
		let newly = record_deposit(&self.deposits, &self.relay, tx_ref, Party::User(user), network, amount)
			.await
			.map_err(|e| WatcherError::Credit(e.to_string()))?;
		if newly {
			info!(user = %user, tx = %transfer.tx_hash, "deposit watcher: credited on-chain USDT deposit");
		}
		Ok(())
	}

	// ── JSON-RPC ────────────────────────────────────────────────────────────────
	async fn block_number(&self) -> Result<u64, WatcherError> {
		let result = self.rpc("eth_blockNumber", json!([])).await?;
		let hex = result.as_str().ok_or_else(|| WatcherError::Rpc("eth_blockNumber: non-string result".into()))?;
		parse_hex_u64(hex).ok_or_else(|| WatcherError::Rpc(format!("eth_blockNumber: unparseable {hex}")))
	}

	async fn get_logs(&self, from: u64, to: u64, addresses: &[Value]) -> Result<Vec<Value>, WatcherError> {
		let params = json!([{
			"fromBlock": format!("0x{from:x}"),
			"toBlock": format!("0x{to:x}"),
			"address": self.config.usdt_contract,
			"topics": [TRANSFER_TOPIC, Value::Null, addresses],
		}]);
		let result = self.rpc("eth_getLogs", params).await?;
		result.as_array().cloned().ok_or_else(|| WatcherError::Rpc("eth_getLogs: result is not an array".into()))
	}

	async fn rpc(&self, method: &str, params: Value) -> Result<Value, WatcherError> {
		let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
		let response: Value = self
			.http
			.post(&self.config.rpc_url)
			.json(&body)
			.send()
			.await
			.map_err(|e| WatcherError::Rpc(format!("{method}: request failed: {e}")))?
			.json()
			.await
			.map_err(|e| WatcherError::Rpc(format!("{method}: bad json: {e}")))?;
		if let Some(err) = response.get("error").filter(|e| !e.is_null()) {
			return Err(WatcherError::Rpc(format!("{method}: rpc error: {err}")));
		}
		response.get("result").cloned().ok_or_else(|| WatcherError::Rpc(format!("{method}: response had no result")))
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

/// One decoded ERC-20 `Transfer` to a watched address.
struct Transfer {
	/// Lowercase `0x…` 20-byte recipient address (the matched deposit address).
	to: String,
	/// Transferred value in raw on-chain units. For BEP20 USDT (18-dp) this equals the
	/// canonical 18-dp base unit 1:1 — no scaling. (TRC20/TON are 6-dp; their watcher,
	/// when added, must scale via the custody edge.)
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
	let to = address_from_topic(topics[2].as_str()?)?;
	let value = u128_from_word(log.get("data")?.as_str()?)?;
	let tx_hash = log.get("transactionHash")?.as_str()?.to_lowercase();
	let log_index = parse_hex_u64(log.get("logIndex")?.as_str()?)?;
	Some(Transfer { to, value, tx_hash, log_index })
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
		assert_eq!(t.to, "0x024da544a76714a3812096e9ef84d40b2c8863e8");
		assert_eq!(t.value, 5_000_000_000_000_000_000); // 5 USDT at 18 dp
		assert_eq!(t.tx_hash, "0xabcdef0000000000000000000000000000000000000000000000000000000001");
		assert_eq!(t.log_index, 2);
		assert_eq!(t.tx_ref(), "0xabcdef0000000000000000000000000000000000000000000000000000000001:2");
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
	fn rpc_host_hides_the_path() {
		assert_eq!(rpc_host("https://rpc.ankr.com/bsc/secret-key"), "rpc.ankr.com");
		assert_eq!(rpc_host("https://bsc-dataseed.binance.org/"), "bsc-dataseed.binance.org");
	}
}
