//! A minimal TronGrid HTTP client for the Tron custody adapter (broadcast), the withdrawal
//! confirmation watcher, the sweep, and the deposit watcher's TRC20 history reads. Tron's
//! "JSON-RPC" is its own `/wallet/*` REST API (not Ethereum JSON-RPC), plus the indexed
//! `/v1/accounts/...` surface for decoded balances and TRC20 transfers — so this mirrors `bsc_rpc`
//! in shape but speaks a different wire. Addresses cross the wire as Base58Check `T…`; amounts and
//! ids are decimal/hex strings. Reading balances off the indexed account view keeps the 0x41/ABI
//! address encoding entirely inside the signer (the only place that decodes Base58Check).

use std::{collections::HashMap, time::Duration};

use serde_json::{Value, json};

/// The recent-block reference + window a Tron transaction must carry (the analogue of the EVM
/// nonce/gas). Fetched from `getnowblock` and handed to the signer.
pub struct RefBlockParams {
	pub ref_block_bytes: String, // hex, 2 bytes
	pub ref_block_hash: String,  // hex, 8 bytes
	pub expiration: i64,         // head ts + window (ms)
	pub timestamp: i64,          // head ts (ms)
}

/// One decoded incoming TRC20 transfer (from the indexed `/v1/accounts/.../transactions/trc20`).
pub struct Trc20Transfer {
	pub transaction_id: String,
	pub to: String,
	pub from: String,
	pub value: u128,
	pub block_timestamp: i64,
	pub token: String,
}

/// A mined transaction's outcome (from `gettransactioninfobyid`). `None` at the call site means
/// the node returned an empty object — not yet mined (or dropped/expired), distinct from an error.
pub struct TronReceipt {
	pub block_number: u64,
	pub success: bool,
}

/// An account's fee + token balances from the indexed account view — one call, no ABI encoding.
pub struct AccountState {
	/// Native TRX balance in SUN (the gas budget).
	pub trx: u128,
	trc20: HashMap<String, u128>,
}
impl AccountState {
	/// The balance of one TRC20 token (`0` if the account holds none / is unactivated).
	pub fn trc20(&self, token: &str) -> u128 {
		self.trc20.get(token).copied().unwrap_or(0)
	}
}

pub struct TronRpc {
	http: reqwest::Client,
	base_url: String,
	api_key: Option<String>,
	/// Seconds added to the head-block timestamp for a transaction's expiration window.
	expiration_secs: i64,
}
impl TronRpc {
	pub fn new(base_url: String, api_key: Option<String>, expiration_secs: i64) -> Self {
		let http = reqwest::Client::builder()
			.timeout(Duration::from_secs(20))
			.build()
			.expect("reqwest client builds with default config");
		Self {
			http,
			base_url: base_url.trim_end_matches('/').to_owned(),
			api_key,
			expiration_secs,
		}
	}

	/// The recent-block reference + validity window, derived from the head block. Tron has no
	/// nonce; this (ref-block + ~60s expiration) is the replay/anti-double-spend anchor.
	pub async fn ref_block_params(&self) -> Result<RefBlockParams, TronRpcError> {
		let block = self.post("/wallet/getnowblock", json!({})).await?;
		let number = block
			.pointer("/block_header/raw_data/number")
			.and_then(Value::as_u64)
			.ok_or_else(|| TronRpcError::Rpc("getnowblock: missing number".into()))?;
		let timestamp = block
			.pointer("/block_header/raw_data/timestamp")
			.and_then(Value::as_i64)
			.ok_or_else(|| TronRpcError::Rpc("getnowblock: missing timestamp".into()))?;
		let block_id = block
			.get("blockID")
			.and_then(Value::as_str)
			.ok_or_else(|| TronRpcError::Rpc("getnowblock: missing blockID".into()))?;
		let (ref_block_bytes, ref_block_hash) = ref_block(number, block_id).ok_or_else(|| TronRpcError::Rpc("getnowblock: malformed blockID".into()))?;
		Ok(RefBlockParams {
			ref_block_bytes,
			ref_block_hash,
			expiration: timestamp + self.expiration_secs * 1000,
			timestamp,
		})
	}

	/// The latest **solidified** (irreversible) block height. The withdrawal watcher settles only
	/// once a broadcast tx's block is at or below this — Tron's finality, the analogue of waiting N
	/// EVM confirmations.
	pub async fn solid_block_number(&self) -> Result<u64, TronRpcError> {
		let block = self.post("/walletsolidity/getnowblock", json!({})).await?;
		block
			.pointer("/block_header/raw_data/number")
			.and_then(Value::as_u64)
			.ok_or_else(|| TronRpcError::Rpc("getnowblock(solid): missing number".into()))
	}

	/// The account's TRX (SUN) + TRC20 balances from the indexed account view. The sweep reads
	/// both: the USDT to move, and the TRX gas budget to move it. Unactivated ⇒ all zero.
	pub async fn account_state(&self, address: &str) -> Result<AccountState, TronRpcError> {
		let response = self.get(&format!("/v1/accounts/{address}")).await?;
		Ok(decode_account_state(&response))
	}

	/// Broadcast a hex-encoded signed `Transaction`. Returns its txid on acceptance. A node-level
	/// rejection (e.g. `DUP_TRANSACTION_ERROR`, `TRANSACTION_EXPIRATION_ERROR`) comes back as
	/// [`TronRpcError::Rpc`] carrying `code: message`, for the caller to interpret — re-sending the
	/// SAME bytes is safe, so a re-broadcast treats `DUP_TRANSACTION_ERROR` as success.
	pub async fn broadcast_hex(&self, signed_tx_hex: &str) -> Result<String, TronRpcError> {
		let result = self.post("/wallet/broadcasthex", json!({ "transaction": signed_tx_hex })).await?;
		if result.get("result").and_then(Value::as_bool) == Some(true) {
			return Ok(result.get("txid").and_then(Value::as_str).unwrap_or_default().to_owned());
		}
		let code = result.get("code").and_then(Value::as_str).unwrap_or("UNKNOWN_ERROR");
		let message = result.get("message").and_then(Value::as_str).map(decode_hex_message).unwrap_or_default();
		Err(TronRpcError::Rpc(format!("{code}: {message}")))
	}

	/// The outcome of a broadcast transaction by txid. An empty object (`{}`) means the node has
	/// no record yet — not mined (or dropped/expired), returned as `None` so the caller waits.
	pub async fn transaction_info(&self, txid: &str) -> Result<Option<TronReceipt>, TronRpcError> {
		let info = self.post("/wallet/gettransactioninfobyid", json!({ "value": txid })).await?;
		Ok(decode_receipt(&info))
	}

	/// Confirmed incoming TRC20 transfers to `address` for `token`, newer than `min_timestamp`
	/// (ms), oldest first. `only_confirmed` returns solidified-only — Tron's irreversible-block
	/// guarantee, the analogue of waiting N EVM confirmations.
	pub async fn incoming_trc20(&self, address: &str, token: &str, min_timestamp: i64, limit: u32) -> Result<Vec<Trc20Transfer>, TronRpcError> {
		let path = format!(
			"/v1/accounts/{address}/transactions/trc20?only_to=true&only_confirmed=true&contract_address={token}&min_timestamp={min_timestamp}&order_by=block_timestamp,asc&limit={limit}"
		);
		let response = self.get(&path).await?;
		let rows = response.get("data").and_then(Value::as_array).cloned().unwrap_or_default();
		Ok(rows.iter().filter_map(decode_trc20_transfer).collect())
	}

	async fn post(&self, path: &str, body: Value) -> Result<Value, TronRpcError> {
		self.send(self.http.post(format!("{}{path}", self.base_url)).json(&body), path).await
	}

	async fn get(&self, path: &str) -> Result<Value, TronRpcError> {
		self.send(self.http.get(format!("{}{path}", self.base_url)), path).await
	}

	async fn send(&self, mut request: reqwest::RequestBuilder, path: &str) -> Result<Value, TronRpcError> {
		if let Some(key) = &self.api_key {
			request = request.header("TRON-PRO-API-KEY", key);
		}
		let response = request.send().await.map_err(|e| TronRpcError::Transport(format!("{path}: request failed: {e}")))?;
		let value: Value = response.json().await.map_err(|e| TronRpcError::Transport(format!("{path}: bad json: {e}")))?;
		// `/wallet` surfaces signal validation errors inline (handled per-method); an explicit
		// top-level `Error` string is always a hard failure.
		if let Some(err) = value.get("Error").and_then(Value::as_str) {
			return Err(TronRpcError::Rpc(format!("{path}: {err}")));
		}
		Ok(value)
	}
}

#[derive(Debug, thiserror::Error)]
pub enum TronRpcError {
	/// No well-formed answer (network, timeout, bad JSON) — nothing happened on-chain, retry.
	#[error("tron rpc transport: {0}")]
	Transport(String),
	/// The node returned an error payload — its `code: message` is carried verbatim so the caller
	/// can interpret it (e.g. a `DUP_TRANSACTION_ERROR` re-broadcast is idempotent success).
	#[error("tron rpc error: {0}")]
	Rpc(String),
}

/// `ref_block_bytes` = the low 2 bytes of the block height; `ref_block_hash` = bytes [8,16) of the
/// 32-byte block id. Both hex. Pulled out of the async call so it is unit-testable without a node.
fn ref_block(number: u64, block_id_hex: &str) -> Option<(String, String)> {
	let ref_block_hash = block_id_hex.get(16..32)?.to_owned();
	if !ref_block_hash.bytes().all(|b| b.is_ascii_hexdigit()) {
		return None;
	}
	Some((format!("{:04x}", (number & 0xffff) as u16), ref_block_hash))
}

/// Decode the indexed `/v1/accounts/{address}` view into the TRX + TRC20 balances. An empty/absent
/// account (unactivated) yields all zeros.
fn decode_account_state(response: &Value) -> AccountState {
	let Some(account) = response.get("data").and_then(Value::as_array).and_then(|a| a.first()) else {
		return AccountState { trx: 0, trc20: HashMap::new() };
	};
	let trx = account.get("balance").and_then(Value::as_u64).map_or(0, u128::from);
	let mut trc20 = HashMap::new();
	if let Some(entries) = account.get("trc20").and_then(Value::as_array) {
		for entry in entries.iter().filter_map(Value::as_object) {
			for (contract, balance) in entry {
				if let Some(value) = balance.as_str().and_then(|s| s.parse::<u128>().ok()) {
					trc20.insert(contract.clone(), value);
				}
			}
		}
	}
	AccountState { trx, trc20 }
}

/// Decode a `gettransactioninfobyid` result. An empty object is not-yet-mined (`None`); a mined
/// receipt yields its block number and whether the contract execution succeeded (`SUCCESS`).
fn decode_receipt(info: &Value) -> Option<TronReceipt> {
	let block_number = info.get("blockNumber").and_then(Value::as_u64)?;
	// A pure-TRX transfer has no `receipt.result`; a contract call does. Absent ⇒ treat as success
	// (the tx is in a block); present ⇒ require SUCCESS (a reverted call moved no tokens).
	let success = info.pointer("/receipt/result").and_then(Value::as_str).is_none_or(|r| r == "SUCCESS");
	Some(TronReceipt { block_number, success })
}

/// Decode one indexed TRC20 transfer row. `None` if the shape is unexpected or the value exceeds
/// `u128` (an impossible USDT amount we refuse to credit).
fn decode_trc20_transfer(row: &Value) -> Option<Trc20Transfer> {
	let value: u128 = row.get("value").and_then(Value::as_str)?.parse().ok()?;
	Some(Trc20Transfer {
		transaction_id: row.get("transaction_id")?.as_str()?.to_owned(),
		to: row.get("to")?.as_str()?.to_owned(),
		from: row.get("from").and_then(Value::as_str).unwrap_or_default().to_owned(),
		value,
		block_timestamp: row.get("block_timestamp").and_then(Value::as_i64)?,
		token: row.pointer("/token_info/address").and_then(Value::as_str).unwrap_or_default().to_owned(),
	})
}

/// TronGrid error messages are hex-encoded — decode to UTF-8 for the log, falling back to the raw.
fn decode_hex_message(message: &str) -> String {
	hex::decode(message).ok().and_then(|b| String::from_utf8(b).ok()).unwrap_or_else(|| message.to_owned())
}

#[cfg(test)]
mod tests {
	use serde_json::json;

	use super::*;

	#[test]
	fn extracts_the_ref_block_from_the_head() {
		// number low 2 bytes = 0x1234; ref_block_hash = chars [16,32) of the 64-hex blockID.
		let block_id = "0000000001234abcdeadbeefcafef00d1111111122222222333333334444444a";
		let (bytes, hash) = ref_block(0x9_1234, block_id).unwrap();
		assert_eq!(bytes, "1234");
		assert_eq!(hash, &block_id[16..32]);
	}

	#[test]
	fn decodes_account_balances() {
		let response = json!({ "data": [ {
			"balance": 30_000_000u64, // 30 TRX in SUN
			"trc20": [ { "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t": "5000000" }, { "TOther": "1" } ]
		} ] });
		let state = decode_account_state(&response);
		assert_eq!(state.trx, 30_000_000);
		assert_eq!(state.trc20("TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t"), 5_000_000); // 5 USDT (6dp)
		assert_eq!(state.trc20("TUnknown"), 0);
		// An unactivated account (no data) is all zeros.
		let empty = decode_account_state(&json!({ "data": [] }));
		assert_eq!(empty.trx, 0);
		assert_eq!(empty.trc20("TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t"), 0);
	}

	#[test]
	fn not_mined_has_no_receipt() {
		assert!(decode_receipt(&json!({})).is_none());
	}

	#[test]
	fn decodes_a_successful_and_reverted_receipt() {
		let ok = decode_receipt(&json!({ "blockNumber": 100, "receipt": { "result": "SUCCESS" } })).unwrap();
		assert_eq!(ok.block_number, 100);
		assert!(ok.success);
		let reverted = decode_receipt(&json!({ "blockNumber": 100, "receipt": { "result": "REVERT" } })).unwrap();
		assert!(!reverted.success);
		// A native TRX transfer has no receipt.result — mined ⇒ success.
		assert!(decode_receipt(&json!({ "blockNumber": 100 })).unwrap().success);
	}

	#[test]
	fn decodes_an_incoming_transfer() {
		let row = json!({
			"transaction_id": "abc123",
			"to": "TJRabPrwbZy45sbavfcjinPJC18kjpRTv8",
			"from": "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t",
			"value": "5000000",
			"block_timestamp": 1700000000000i64,
			"token_info": { "address": "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t" }
		});
		let t = decode_trc20_transfer(&row).unwrap();
		assert_eq!(t.transaction_id, "abc123");
		assert_eq!(t.value, 5_000_000); // 5 USDT at 6dp
		assert_eq!(t.to, "TJRabPrwbZy45sbavfcjinPJC18kjpRTv8");
	}

	#[test]
	fn decodes_a_hex_error_message() {
		assert_eq!(decode_hex_message(&hex::encode("balance is not sufficient")), "balance is not sufficient");
		assert_eq!(decode_hex_message("not hex zz"), "not hex zz");
	}
}
