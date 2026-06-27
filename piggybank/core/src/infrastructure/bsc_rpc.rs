//! A minimal BSC JSON-RPC client for the chain custody adapter (broadcasts) and the
//! withdrawal confirmation watcher — the handful of methods they need over HTTP. The
//! deposit watcher keeps its own read-only client; this one carries the write-path calls
//! (nonce, gas price, send raw transaction) plus the confirmation reads (block number,
//! transaction receipt).

use std::time::Duration;

use serde_json::{Value, json};

pub struct BscRpc {
	http: reqwest::Client,
	url: String,
}
impl BscRpc {
	pub fn new(url: String) -> Self {
		let http = reqwest::Client::builder()
			.timeout(Duration::from_secs(20))
			.build()
			.expect("reqwest client builds with default config");
		Self { http, url }
	}

	/// The account's next nonce, counting pending (mempool) transactions.
	pub async fn pending_nonce(&self, address: &str) -> Result<u64, RpcError> {
		let value = self.call("eth_getTransactionCount", json!([address, "pending"])).await?;
		hex_to_u64(as_str(&value, "eth_getTransactionCount")?).ok_or_else(|| RpcError::Rpc("eth_getTransactionCount: unparseable".into()))
	}

	/// The account's nonce counting only MINED transactions. The sweep signs a deposit
	/// address's USDT transfer at this nonce so a re-sign of an unconfirmed sweep is the same
	/// deterministic transaction (RFC-6979) — an idempotent re-broadcast, never a second tx
	/// at the next nonce that would double-send.
	pub async fn latest_nonce(&self, address: &str) -> Result<u64, RpcError> {
		let value = self.call("eth_getTransactionCount", json!([address, "latest"])).await?;
		hex_to_u64(as_str(&value, "eth_getTransactionCount")?).ok_or_else(|| RpcError::Rpc("eth_getTransactionCount: unparseable".into()))
	}

	pub async fn gas_price(&self) -> Result<u128, RpcError> {
		let value = self.call("eth_gasPrice", json!([])).await?;
		hex_to_u128(as_str(&value, "eth_gasPrice")?).ok_or_else(|| RpcError::Rpc("eth_gasPrice: unparseable".into()))
	}

	/// The latest block height — the head the withdrawal watcher measures confirmation
	/// depth against.
	pub async fn block_number(&self) -> Result<u64, RpcError> {
		let value = self.call("eth_blockNumber", json!([])).await?;
		hex_to_u64(as_str(&value, "eth_blockNumber")?).ok_or_else(|| RpcError::Rpc("eth_blockNumber: unparseable".into()))
	}

	/// The receipt for a broadcast transaction. `None` means it is not yet mined (the node
	/// returns a JSON `null` result) — distinct from an error, so the watcher just waits.
	/// A mined receipt carries its block height (for the confirmation-depth check) and its
	/// `status` (`true` = success, `false` = reverted — the funds did not move).
	pub async fn transaction_receipt(&self, tx_hash: &str) -> Result<Option<TxReceipt>, RpcError> {
		let value = self.call("eth_getTransactionReceipt", json!([tx_hash])).await?;
		decode_receipt(&value)
	}

	/// The account's native (BNB) balance in wei — the gas the sweep checks before moving a
	/// deposit address's USDT (and the gas station tops up when it is short).
	pub async fn bnb_balance(&self, address: &str) -> Result<u128, RpcError> {
		let value = self.call("eth_getBalance", json!([address, "latest"])).await?;
		hex_to_u128(as_str(&value, "eth_getBalance")?).ok_or_else(|| RpcError::Rpc("eth_getBalance: unparseable".into()))
	}

	/// An ERC-20 `balanceOf(address)` via `eth_call` — the token units the address holds,
	/// the amount the sweep moves to the treasury.
	pub async fn erc20_balance(&self, token: &str, address: &str) -> Result<u128, RpcError> {
		let data = balance_of_calldata(address).ok_or_else(|| RpcError::Rpc("erc20_balance: address is not 0x 20-byte".into()))?;
		let value = self.call("eth_call", json!([{ "to": token, "data": data }, "latest"])).await?;
		word_to_u128(as_str(&value, "eth_call")?).ok_or_else(|| RpcError::Rpc("eth_call balanceOf: unparseable result".into()))
	}

	/// Submit a raw signed transaction; returns its hash on acceptance. A node-level error
	/// (e.g. "already known", "nonce too low", "insufficient funds") comes back as
	/// [`RpcError::Rpc`] for the caller to interpret — sending the SAME signed transaction
	/// twice is safe, so a re-broadcast treats "already known" as success.
	pub async fn send_raw_transaction(&self, raw_tx_hex: &str) -> Result<String, RpcError> {
		let value = self.call("eth_sendRawTransaction", json!([raw_tx_hex])).await?;
		as_str(&value, "eth_sendRawTransaction").map(str::to_owned)
	}

	async fn call(&self, method: &str, params: Value) -> Result<Value, RpcError> {
		let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
		let response: Value = self
			.http
			.post(&self.url)
			.json(&body)
			.send()
			.await
			.map_err(|e| RpcError::Transport(format!("{method}: request failed: {e}")))?
			.json()
			.await
			.map_err(|e| RpcError::Transport(format!("{method}: bad json: {e}")))?;
		if let Some(err) = response.get("error").filter(|e| !e.is_null()) {
			return Err(RpcError::Rpc(format!("{method}: {err}")));
		}
		response.get("result").cloned().ok_or_else(|| RpcError::Rpc(format!("{method}: response had no result")))
	}
}

/// A mined transaction's receipt — the block it landed in (confirmation-depth measurement)
/// and whether it succeeded (`false` = reverted, so no balance moved).
pub struct TxReceipt {
	pub block_number: u64,
	pub success: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum RpcError {
	/// No well-formed answer (network, timeout, bad JSON) — nothing happened on-chain, retry.
	#[error("rpc transport: {0}")]
	Transport(String),
	/// The node returned a JSON-RPC error object — its message is carried verbatim so the
	/// caller can interpret it (e.g. an "already known" broadcast is idempotent success).
	#[error("rpc error: {0}")]
	Rpc(String),
}

/// Decode an `eth_getTransactionReceipt` result. A JSON `null` (the node's answer for a
/// not-yet-mined tx) is `Ok(None)`; a mined receipt yields its block height and success
/// flag. Pulled out of the async call so it is unit-testable without a node.
fn decode_receipt(value: &Value) -> Result<Option<TxReceipt>, RpcError> {
	if value.is_null() {
		return Ok(None); // not yet mined
	}
	let block_number = value
		.get("blockNumber")
		.and_then(Value::as_str)
		.and_then(hex_to_u64)
		.ok_or_else(|| RpcError::Rpc("eth_getTransactionReceipt: missing/invalid blockNumber".into()))?;
	// Byzantium-onward receipts carry a `status` (`0x1`/`0x0`); BSC has always had it.
	let success = value
		.get("status")
		.and_then(Value::as_str)
		.map(|s| s == "0x1")
		.ok_or_else(|| RpcError::Rpc("eth_getTransactionReceipt: missing status".into()))?;
	Ok(Some(TxReceipt { block_number, success }))
}

fn as_str<'a>(value: &'a Value, method: &str) -> Result<&'a str, RpcError> {
	value.as_str().ok_or_else(|| RpcError::Rpc(format!("{method}: non-string result")))
}

fn hex_to_u64(s: &str) -> Option<u64> {
	u64::from_str_radix(s.strip_prefix("0x")?, 16).ok()
}

fn hex_to_u128(s: &str) -> Option<u128> {
	u128::from_str_radix(s.strip_prefix("0x")?, 16).ok()
}

/// `balanceOf(address)` calldata: selector `0x70a08231` + the 32-byte left-padded address.
/// `None` if `address` is not a 0x 20-byte hex address.
fn balance_of_calldata(address: &str) -> Option<String> {
	let hex = address.strip_prefix("0x").unwrap_or(address);
	if hex.len() != 40 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
		return None;
	}
	Some(format!("0x70a08231{hex:0>64}"))
}

/// A 32-byte big-endian uint256 `eth_call` result → `u128`. `None` if it is malformed or
/// exceeds `u128` (the high 16 bytes are non-zero) — refused rather than truncated, like the
/// deposit watcher's value decode.
fn word_to_u128(word: &str) -> Option<u128> {
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

#[cfg(test)]
mod tests {
	use serde_json::json;

	use super::decode_receipt;

	#[test]
	fn pending_transaction_has_no_receipt() {
		// The node answers a not-yet-mined tx with a null result — wait, don't error.
		assert!(decode_receipt(&json!(null)).unwrap().is_none());
	}

	#[test]
	fn decodes_a_successful_receipt() {
		let receipt = decode_receipt(&json!({ "blockNumber": "0x1b4", "status": "0x1" })).unwrap().expect("mined");
		assert_eq!(receipt.block_number, 436);
		assert!(receipt.success);
	}

	#[test]
	fn decodes_a_reverted_receipt() {
		// A reverted transfer moved no funds — `success` is false, so the watcher alerts
		// rather than settling.
		let receipt = decode_receipt(&json!({ "blockNumber": "0x10", "status": "0x0" })).unwrap().expect("mined");
		assert!(!receipt.success);
	}

	#[test]
	fn rejects_a_receipt_without_status() {
		assert!(decode_receipt(&json!({ "blockNumber": "0x10" })).is_err());
	}

	#[test]
	fn builds_balance_of_calldata() {
		// selector 0x70a08231 + 24 zero nibbles of left-pad + the 40-hex address.
		assert_eq!(
			super::balance_of_calldata("0x024da544a76714a3812096e9ef84d40b2c8863e8").unwrap(),
			"0x70a08231000000000000000000000000024da544a76714a3812096e9ef84d40b2c8863e8"
		);
		assert!(super::balance_of_calldata("0x1234").is_none());
	}

	#[test]
	fn decodes_a_balanceof_word() {
		// 5 USDT (5e18) as a 32-byte word.
		assert_eq!(
			super::word_to_u128("0x0000000000000000000000000000000000000000000000004563918244f40000"),
			Some(5_000_000_000_000_000_000)
		);
		assert_eq!(super::word_to_u128("0x0000000000000000000000000000000000000000000000000000000000000000"), Some(0));
		// Empty (`0x`, a non-contract call) and over-u128 are refused, not truncated.
		assert!(super::word_to_u128("0x").is_none());
		assert!(super::word_to_u128("0x0000000000000000000000000000000100000000000000000000000000000000").is_none());
	}
}
