//! The custody/signing-service port — the narrow "broadcast this withdrawal" seam.
//!
//! Custody is a **separate trust domain** (MPC/HSM): it holds the private keys,
//! applies its own policy engine (limits, allowlists, velocity, 4-eyes), and is the
//! second gate even if the hub is compromised. The hub never signs. This port is all
//! the hub asks of it — submit an *already-reserved* withdrawal for on-chain
//! broadcast, **idempotently by `withdrawal_id`** (a retried relay delivery must not
//! double-send). A stub adapter stands in until the real custody service exists.

use async_trait::async_trait;
use domain::{
	architecture::Gateway,
	money::{Network, Usdt, WalletAddress},
};
use thiserror::Error;
use uuid::Uuid;

/// A [`Gateway`]: an external transactional system that owns its own atomicity —
/// by construction it can never enrol in a Postgres transaction.
#[async_trait]
pub trait Custody: Gateway {
	/// Submit the withdrawal's on-chain leg for signing + broadcast. MUST be
	/// idempotent by `request.withdrawal_id` so an at-least-once relay never
	/// double-spends.
	async fn broadcast(&self, request: &BroadcastRequest) -> Result<(), CustodyError>;

	/// The rail treasury's spendable **on-chain** USDT, in 18-dp base units — the
	/// withdrawal **dispatch gate**'s source. The ledger's `wallet:<net>` balance is
	/// accounting (it counts un-swept deposit-address funds the treasury cannot spend),
	/// so dispatchability is `min(TB rail, this)`. `None` means the adapter has no chain
	/// view (the stub / an unwired rail) — callers fall back to the TB-only behaviour.
	/// Read-only: never signs, never allocates a nonce/seqno; the adapter's broadcast-time
	/// `ensure_treasury_funded` remains the last-line backstop.
	async fn treasury_liquidity(&self, network: Network) -> Result<Option<Usdt>, CustodyError> {
		let _ = network;
		Ok(None)
	}

	/// The rail treasury's operator **funding view** — the hot-wallet address plus its
	/// real on-chain USDT and native-coin gas balances, for the treasury screen. Same
	/// read-only rules as [`treasury_liquidity`](Custody::treasury_liquidity); `None`
	/// means the adapter has no chain view (the stub / an unwired rail).
	async fn treasury_funding(&self, network: Network) -> Result<Option<TreasuryFunding>, CustodyError> {
		let _ = network;
		Ok(None)
	}
}
/// A rail treasury's funding view, read live from the chain: where the operator funds
/// (`address`) and what is actually there. The balance fields degrade to `None` when
/// their chain read fails — the address alone is still useful.
#[derive(Debug, Clone)]
pub struct TreasuryFunding {
	/// The treasury hot wallet — fund USDT (liquidity) + native coin (gas) here.
	pub address: String,
	/// On-chain USDT held by the treasury wallet (canonical 18-dp).
	pub onchain_usdt: Option<Usdt>,
	/// Native-coin gas balance, pre-rendered in whole units (BNB/TRX/TON differ in
	/// decimals, so the adapter formats it — see [`format_native_units`]).
	pub onchain_gas: Option<String>,
}
/// Render a native-coin amount (`units` at `decimals`) as a decimal string with the
/// fraction's trailing zeros trimmed — the gas-balance display format.
/// [`Usdt::to_decimal_string`] is fixed at the 18-dp canonical scale; gas is 18 (wei) /
/// 6 (SUN) / 9 (nanoton) dp per rail.
pub fn format_native_units(units: u128, decimals: u32) -> String {
	let scale = 10u128.pow(decimals);
	let int = units / scale;
	let frac = units % scale;
	if frac == 0 {
		return int.to_string();
	}
	let frac = format!("{frac:0width$}", width = decimals as usize);
	let frac = frac.trim_end_matches('0');
	format!("{int}.{frac}")
}
/// A request to broadcast the on-chain leg of a withdrawal. `withdrawal_id` is the
/// idempotency key the custodian MUST dedupe on.
#[derive(Debug, Clone)]
pub struct BroadcastRequest {
	pub withdrawal_id: Uuid,
	pub network: Network,
	pub address: WalletAddress,
	/// The net amount to send on-chain (gross minus the retained fee).
	pub amount: Usdt,
}

/// Failure modes the relay distinguishes — transient (retry; nothing was sent) vs a
/// policy/liquidity refusal (park for intervention; the reservation stays pending).
#[derive(Debug, Error)]
pub enum CustodyError {
	#[error("custody unavailable: {0}")]
	Unavailable(String),
	#[error("custody rejected: {0}")]
	Rejected(String),
}

#[cfg(test)]
mod tests {
	use super::format_native_units;

	#[test]
	fn formats_native_units_across_gas_scales() {
		assert_eq!(format_native_units(0, 18), "0");
		assert_eq!(format_native_units(1_500_000_000_000_000_000, 18), "1.5");
		assert_eq!(format_native_units(2_000_000, 6), "2");
		assert_eq!(format_native_units(123_456, 6), "0.123456");
		assert_eq!(format_native_units(1_000_000_001, 9), "1.000000001");
	}
}
