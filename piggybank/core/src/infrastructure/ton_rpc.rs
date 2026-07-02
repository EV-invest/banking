//! A minimal toncenter v3 REST client for the TON custody adapter, the jetton deposit
//! watcher, the withdrawal confirmation watcher, and the sweep — the handful of HTTP
//! calls they need. The analog of [`bsc_rpc`](super::bsc_rpc), but TON is a REST +
//! indexer chain (no `eth_*` JSON-RPC, no logs): deposits come pre-decoded from the
//! indexer's `/jetton/transfers`, the wallet nonce is a `seqno` get-method, and a signed
//! message is POSTed as a base64 BoC.
//!
//! Read paths return [`RpcError::Transport`] on a network/JSON failure (nothing happened,
//! retry) and [`RpcError::Rpc`] on a well-formed error answer. The pure decode helpers are
//! split out so they are unit-testable without a live indexer.

use std::time::Duration;

use serde_json::{Value, json};

pub struct TonRpc {
	http: reqwest::Client,
	base_url: String,
	api_key: Option<String>,
}

/// One incoming jetton transfer the deposit watcher credits.
#[derive(Debug, Clone, PartialEq)]
pub struct JettonDeposit {
	/// The transaction hash — the stable idempotency key for [`record_deposit`].
	pub tx_hash: String,
	/// Jetton base units (6-dp on-chain) received.
	pub amount: u128,
	/// The transaction's unix time — the deposit watcher's globally-comparable cursor.
	pub now: u64,
}

/// A user/treasury jetton wallet's address + balance (one `/jetton/wallets` row).
#[derive(Debug, Clone, PartialEq)]
pub struct JettonWallet {
	pub address: String,
	pub balance: u128,
}

impl TonRpc {
	pub fn new(base_url: String, api_key: Option<String>) -> Self {
		let http = reqwest::Client::builder()
			.timeout(Duration::from_secs(20))
			.build()
			.expect("reqwest client builds with default config");
		Self {
			http,
			base_url: base_url.trim_end_matches('/').to_owned(),
			api_key,
		}
	}

	/// The wallet contract's `seqno` (its nonce). A not-yet-deployed wallet has no
	/// get-methods, so a failed/non-zero `exit_code` is reported as seqno `0` — its first
	/// outgoing message both deploys it (StateInit attached) and runs at seqno 0.
	pub async fn seqno(&self, address: &str) -> Result<u64, RpcError> {
		let body = json!({ "address": address, "method": "seqno", "stack": [] });
		let value = self.post("runGetMethod", &body).await?;
		Ok(decode_seqno(&value))
	}

	/// The sender's jetton wallet (address + jetton balance) for `owner` under `master`,
	/// from the indexer's decoded `/jetton/wallets`. `None` if the owner has no jetton
	/// wallet yet (never received this jetton).
	pub async fn jetton_wallet(&self, owner: &str, master: &str) -> Result<Option<JettonWallet>, RpcError> {
		let query = [("owner_address", owner), ("jetton_address", master), ("limit", "1")];
		let value = self.get("jetton/wallets", &query).await?;
		Ok(decode_jetton_wallet(&value))
	}

	/// Incoming jetton transfers to `owner` (decoded by the indexer), at or after the
	/// `start_now` unix-time watermark, oldest first. The deposit watcher's scan call.
	pub async fn incoming_jetton_transfers(&self, owner: &str, master: &str, start_now: u64, limit: u32) -> Result<Vec<JettonDeposit>, RpcError> {
		let start = start_now.to_string();
		let limit = limit.to_string();
		let query = [
			("owner_address", owner),
			("jetton_master", master),
			("direction", "in"),
			("start_utime", start.as_str()),
			("sort", "asc"),
			("limit", limit.as_str()),
		];
		// Attribution is entirely the server-side `owner_address` + `jetton_master` filter, so
		// core never decodes a TON address.
		let value = self.get("jetton/transfers", &query).await?;
		Ok(decode_jetton_transfers(&value))
	}

	/// Outgoing jetton transfers FROM `owner` (decoded by the indexer), at or after the
	/// `start_now` unix-time watermark, oldest first. The withdrawal watcher's settlement
	/// proof: a treasury seqno advance only means the wallet processed *an* external message,
	/// not that the internal jetton transfer landed (a bounce advances the seqno too). The
	/// indexer only surfaces transfers whose transaction was not aborted, so a matching
	/// non-aborted outgoing transfer of the expected amount is positive proof the USDT
	/// actually left — the mirror of the deposit path, reusing the same tested decoder.
	pub async fn outgoing_jetton_transfers(&self, owner: &str, master: &str, start_now: u64, limit: u32) -> Result<Vec<JettonDeposit>, RpcError> {
		let start = start_now.to_string();
		let limit = limit.to_string();
		let query = [
			("owner_address", owner),
			("jetton_master", master),
			("direction", "out"),
			("start_utime", start.as_str()),
			("sort", "asc"),
			("limit", limit.as_str()),
		];
		let value = self.get("jetton/transfers", &query).await?;
		Ok(decode_jetton_transfers(&value))
	}

	/// The account's native Toncoin balance in nanotons (the gas the sweep checks before a
	/// jetton move, and the gas station tops up when short).
	pub async fn balance(&self, address: &str) -> Result<u128, RpcError> {
		let query = [("address", address)];
		let value = self.get("accountStates", &query).await?;
		decode_balance(&value).ok_or_else(|| RpcError::Rpc("accountStates: missing/invalid balance".into()))
	}

	/// Submit a signed external message (base64 BoC). toncenter accepts the same bytes
	/// twice (a re-broadcast is a no-op once the wallet's seqno has advanced), so a retry
	/// of the SAME BoC never double-sends.
	pub async fn send_message(&self, boc_base64: &str) -> Result<(), RpcError> {
		let body = json!({ "boc": boc_base64 });
		self.post("message", &body).await.map(|_| ())
	}

	async fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Value, RpcError> {
		let url = format!("{}/{path}", self.base_url);
		let mut request = self.http.get(&url).query(query);
		if let Some(key) = &self.api_key {
			request = request.header("X-Api-Key", key);
		}
		let response = request.send().await.map_err(|e| RpcError::Transport(format!("{path}: request failed: {e}")))?;
		Self::read_json(path, response).await
	}

	async fn post(&self, path: &str, body: &Value) -> Result<Value, RpcError> {
		let url = format!("{}/{path}", self.base_url);
		let mut request = self.http.post(&url).json(body);
		if let Some(key) = &self.api_key {
			request = request.header("X-Api-Key", key);
		}
		let response = request.send().await.map_err(|e| RpcError::Transport(format!("{path}: request failed: {e}")))?;
		Self::read_json(path, response).await
	}

	/// A non-2xx toncenter response carries an `error`/`code` body — surface it as a
	/// well-formed [`RpcError::Rpc`] (the message is verbatim, so a caller can interpret an
	/// "already known"-style send). A body that won't parse as JSON is a transport failure.
	async fn read_json(path: &str, response: reqwest::Response) -> Result<Value, RpcError> {
		let status = response.status();
		let value: Value = response.json().await.map_err(|e| RpcError::Transport(format!("{path}: bad json: {e}")))?;
		if !status.is_success() {
			let detail = value.get("error").and_then(Value::as_str).map(str::to_owned).unwrap_or_else(|| value.to_string());
			return Err(RpcError::Rpc(format!("{path}: {status}: {detail}")));
		}
		Ok(value)
	}
}

#[derive(Debug, thiserror::Error)]
pub enum RpcError {
	/// No well-formed answer (network, timeout, bad JSON) — nothing happened, retry.
	#[error("ton rpc transport: {0}")]
	Transport(String),
	/// toncenter returned an error body — the message is carried verbatim for the caller.
	#[error("ton rpc error: {0}")]
	Rpc(String),
}

/// Decode a `runGetMethod` "seqno" result. A successful call has `exit_code == 0` and a
/// single num on the stack; anything else (undeployed wallet, missing stack) is seqno `0`.
fn decode_seqno(value: &Value) -> u64 {
	// Nested under `result` on some gateways, flat on others — accept both.
	let root = value.get("result").unwrap_or(value);
	if root.get("exit_code").and_then(Value::as_i64).is_some_and(|c| c != 0) {
		return 0;
	}
	root.get("stack")
		.and_then(Value::as_array)
		.and_then(|stack| stack.first())
		.and_then(stack_item_to_u64)
		.unwrap_or(0)
}

/// A toncenter stack item → `u64`. A num item is `{"type":"num","value":"0x14"}` (hex) on
/// v3; tolerate a bare decimal string or number too.
fn stack_item_to_u64(item: &Value) -> Option<u64> {
	let raw = item.get("value").or(Some(item))?;
	if let Some(n) = raw.as_u64() {
		return Some(n);
	}
	let s = raw.as_str()?.trim();
	match s.strip_prefix("0x") {
		Some(hex) => u64::from_str_radix(hex, 16).ok(),
		None => s.parse::<u64>().ok(),
	}
}

/// Decode the first `/jetton/wallets` row into `(address, balance)`.
fn decode_jetton_wallet(value: &Value) -> Option<JettonWallet> {
	let row = value.get("jetton_wallets")?.as_array()?.first()?;
	let address = row.get("address")?.as_str()?.to_owned();
	let balance = parse_u128(row.get("balance")?)?;
	Some(JettonWallet { address, balance })
}

/// Decode the indexer's `/jetton/transfers` array into credited deposits. Attribution is the
/// server-side `owner_address` + `jetton_master` filter (so core never decodes a TON
/// address); here we only skip aborted transactions, zero-value transfers, and malformed
/// rows — a bad row is dropped rather than failing the whole scan.
fn decode_jetton_transfers(value: &Value) -> Vec<JettonDeposit> {
	let Some(rows) = value.get("jetton_transfers").and_then(Value::as_array) else {
		return Vec::new();
	};
	rows.iter().filter_map(decode_one_transfer).collect()
}

fn decode_one_transfer(row: &Value) -> Option<JettonDeposit> {
	if row.get("transaction_aborted").and_then(Value::as_bool).unwrap_or(false) {
		return None;
	}
	let amount = parse_u128(row.get("amount")?)?;
	if amount == 0 {
		return None;
	}
	let tx_hash = row.get("transaction_hash")?.as_str()?.to_owned();
	let now = row.get("transaction_now").and_then(Value::as_u64).unwrap_or(0);
	Some(JettonDeposit { tx_hash, amount, now })
}

/// The account's native balance (nanotons) from an `/accountStates` response.
fn decode_balance(value: &Value) -> Option<u128> {
	let accounts = value.get("accounts")?.as_array()?;
	let row = accounts.first()?;
	parse_u128(row.get("balance")?)
}

/// A JSON amount that may be a decimal string or a number → `u128`.
fn parse_u128(value: &Value) -> Option<u128> {
	if let Some(n) = value.as_u64() {
		return Some(n as u128);
	}
	value.as_str()?.trim().parse::<u128>().ok()
}

#[cfg(test)]
mod tests {
	use serde_json::json;

	use super::*;

	#[test]
	fn decodes_seqno_from_a_hex_stack() {
		let value = json!({ "exit_code": 0, "stack": [{ "type": "num", "value": "0x14" }] });
		assert_eq!(decode_seqno(&value), 20);
	}

	#[test]
	fn undeployed_or_failed_get_method_is_seqno_zero() {
		// A non-zero exit_code (undeployed wallet) ⇒ seqno 0 (first send deploys it).
		assert_eq!(decode_seqno(&json!({ "exit_code": -13, "stack": [] })), 0);
		// Missing stack ⇒ 0, never a panic.
		assert_eq!(decode_seqno(&json!({ "exit_code": 0 })), 0);
		// Nested under `result`, decimal value.
		assert_eq!(decode_seqno(&json!({ "result": { "exit_code": 0, "stack": [{ "value": "7" }] } })), 7);
	}

	#[test]
	fn decodes_incoming_jetton_transfers() {
		let value = json!({
			"jetton_transfers": [
				{ "amount": "5000000", "transaction_hash": "hash-a", "transaction_now": 1700000000, "transaction_aborted": false },
				// aborted → skipped
				{ "amount": "1000000", "transaction_hash": "hash-b", "transaction_now": 1700000001, "transaction_aborted": true },
				// zero-value → skipped
				{ "amount": "0", "transaction_hash": "hash-c", "transaction_now": 1700000002 },
				// malformed (no hash) → skipped
				{ "amount": "9", "transaction_now": 1700000003 }
			]
		});
		let out = decode_jetton_transfers(&value);
		assert_eq!(out.len(), 1);
		assert_eq!(
			out[0],
			JettonDeposit {
				tx_hash: "hash-a".into(),
				amount: 5_000_000,
				now: 1_700_000_000
			}
		);
	}

	#[test]
	fn decodes_a_jetton_wallet_row() {
		let value = json!({ "jetton_wallets": [{ "address": "0:dead", "balance": "12345678" }] });
		assert_eq!(
			decode_jetton_wallet(&value),
			Some(JettonWallet {
				address: "0:dead".into(),
				balance: 12_345_678
			})
		);
		assert_eq!(decode_jetton_wallet(&json!({ "jetton_wallets": [] })), None);
	}

	#[test]
	fn decodes_account_balance() {
		assert_eq!(decode_balance(&json!({ "accounts": [{ "balance": "100000000" }] })), Some(100_000_000));
		assert_eq!(decode_balance(&json!({ "accounts": [] })), None);
	}
}
