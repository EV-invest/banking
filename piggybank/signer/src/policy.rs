//! The signer's independent spend policy — the second gate that holds even if the hub is
//! compromised.
//!
//! The signer is a distinct trust domain: it holds the keys and applies its OWN limits, so
//! an attacker who owns the hub still cannot make it sign an arbitrary payout. Three
//! controls, two of them opt-in and one always on:
//!
//!   - a **per-transfer USDT cap** — a single signed treasury transfer can move at most this
//!     much, so one forged request can't drain the hot wallet;
//!   - an optional **destination allowlist** — when set, treasury transfers may only go to
//!     pre-registered addresses (a hardened/staged posture; off by default, since the normal
//!     withdrawal model sends to arbitrary user addresses). A TON jetton transfer names two
//!     more addresses that receive native Toncoin, and both are held to the same list: the
//!     `response_destination` (the excess returns there — always the sending wallet itself on
//!     a legitimate withdrawal, so that is accepted with or without a list) and
//!     `our_jetton_wallet` (the internal message's destination, which receives `msg_value`
//!     — pinned with `SIGNER_TON_TREASURY_JETTON_WALLET`, or else it must be on the list);
//!   - a **fee budget** ([`FeeBudget`]) — a ceiling on the gas/fee side of EVERY signed
//!     transaction. The amount caps above bound what leaves in USDT; they say nothing about
//!     the native coin a transaction burns as fee. Without this gate a forged request moving
//!     1 USDT with an absurd `gas_price` would hand the whole native balance to the miner,
//!     and could do so once per nonce.
//!
//! The cap and the allowlist apply only to transfers signed FROM the **treasury** wallet (the
//! withdrawal drain vector); sweeps *into* the treasury and gas top-ups (signed from the
//! separate gas-station wallet) are not treasury spends. Native (gas-coin) transfers from the
//! treasury honor the allowlist too, but not the cap — it is USDT-denominated and cannot price
//! a native amount. Both are **no-ops until configured** (`SIGNER_MAX_TRANSFER_USDT`,
//! `SIGNER_DESTINATION_ALLOWLIST`), so dev/CI and existing deployments are unaffected until
//! an operator opts in — the same convention as the observability seams. An operator enabling
//! the allowlist must either pin the treasury's jetton wallet or put it on the list, or every
//! TON withdrawal is refused.
//!
//! The fee budget is the opposite: **on by default** with ceilings an honest hub never
//! reaches, enforced on every wallet class (treasury, deposit addresses, the gas station) and
//! in every signing handler before a digest is signed. An operator may raise a ceiling via
//! its `SIGNER_MAX_*` variable, but cannot switch it off — `0` is a boot error, not "disabled".
//!
//! `Status` is tonic's large error type we don't control (same as the service handlers).
#![allow(clippy::result_large_err)]

use std::{collections::HashSet, str::FromStr as _};

use domain::money::{Network, Usdt};
use tonic::Status;

use crate::provision;

/// Canonical base units per whole USDT (the domain's 18-dp representation).
const CANONICAL_PER_USDT: u128 = 1_000_000_000_000_000_000;

/// Wei per gwei — the operator configures EVM gas prices in gwei, the wire carries wei.
const WEI_PER_GWEI: u128 = 1_000_000_000;

/// The default EVM gas-price ceilings, in gwei (reasoning on [`FeeBudget`]).
const DEFAULT_MAX_GAS_PRICE_GWEI_BEP20: u64 = 100;
const DEFAULT_MAX_GAS_PRICE_GWEI_POLYGON: u64 = 5_000;

/// The signer's spend policy, loaded once at boot and consulted on every signing request.
#[derive(Clone, Debug, Default)]
pub struct SignerPolicy {
	/// Max USDT (whole units) a single treasury transfer may move. `None` ⇒ uncapped.
	max_transfer_usdt: Option<u64>,
	/// If non-empty, a treasury transfer's destination must be one of these (verbatim wire
	/// address strings). Empty ⇒ any destination is allowed (the default withdrawal model).
	destination_allowlist: HashSet<String>,
	/// The treasury's own USDT jetton wallet, when pinned: a treasury jetton transfer's
	/// `our_jetton_wallet` must be this address. `None` ⇒ it falls back to the allowlist.
	ton_treasury_jetton_wallet: Option<String>,
	/// The always-on ceiling on the fee side of every signed transaction.
	fee_budget: FeeBudget,
}

/// Ceilings on the fee side of a signed transaction, per rail.
///
/// Each mirrors the hub's own configured value (`piggybank/core/src/config.rs`) where the hub
/// has one, so an honest hub is never refused; where the hub has none — the EVM gas price
/// comes straight from `eth_gasPrice` — the signer introduces the ceiling, since the EVM fee
/// is `gas_price × gas_limit` and bounding one factor alone bounds nothing.
///
/// Defaults, and why:
///   - `gas_limit` 100_000 — the hub's `BSC_GAS_LIMIT`/`POLYGON_GAS_LIMIT` default; a USDT
///     transfer is ~50–65k, a plain value transfer 21k.
///   - BSC gas price 100 gwei — normal is 1–5 gwei, so this is a 20× headroom; with the
///     gas-limit cap one transaction burns at most 0.01 BNB.
///   - Polygon gas price 5_000 gwei — normal is 30–300 gwei with spikes to ~1_000; with the
///     gas-limit cap one transaction burns at most 0.5 POL.
///   - Tron `fee_limit` 100_000_000 SUN (100 TRX) — the hub's `TRON_FEE_LIMIT` default.
///   - TON `msg_value` 100_000_000 nanoton (0.1 TON) and `forward_ton_amount` 50_000_000 —
///     the hub's `TON_MSG_VALUE`/`TON_FORWARD_TON_AMOUNT` defaults; the forward amount is
///     paid out of `msg_value`, so it is additionally required not to exceed it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeeBudget {
	max_gas_limit: u64,
	max_gas_price_wei_bep20: u128,
	max_gas_price_wei_polygon: u128,
	max_tron_fee_limit_sun: u64,
	max_ton_msg_value_nano: u64,
	max_ton_forward_nano: u64,
}

impl Default for FeeBudget {
	fn default() -> Self {
		Self {
			max_gas_limit: 100_000,
			max_gas_price_wei_bep20: gwei_to_wei(DEFAULT_MAX_GAS_PRICE_GWEI_BEP20),
			max_gas_price_wei_polygon: gwei_to_wei(DEFAULT_MAX_GAS_PRICE_GWEI_POLYGON),
			max_tron_fee_limit_sun: 100_000_000,
			max_ton_msg_value_nano: 100_000_000,
			max_ton_forward_nano: 50_000_000,
		}
	}
}

/// The caller-supplied fee side of a transaction about to be signed, as it will be signed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FeeQuote {
	/// A legacy EVM transaction: the fee is `gas_price × gas_limit` wei.
	Evm { gas_price: u128, gas_limit: u64 },
	/// A Tron contract call: `fee_limit` is the SUN cap the chain may burn for it.
	Tron { fee_limit: i64 },
	/// A TON jetton transfer: `msg_value` nanotons are attached to the internal message
	/// (the gas budget), of which `forward_ton_amount` is forwarded to the recipient.
	Ton { msg_value: u64, forward_ton_amount: u64 },
}

impl FeeBudget {
	/// Read the ceilings from `lookup` (the environment in production), each falling back to
	/// its default when the variable is unset or empty. A value that is `0` or does not parse
	/// is an error: the budget is not optional, so a typo must fail the boot rather than
	/// silently disable the gate.
	fn from_lookup(lookup: &impl Fn(&str) -> Option<String>) -> color_eyre::Result<Self> {
		let defaults = Self::default();
		Ok(Self {
			max_gas_limit: env_cap(lookup, "SIGNER_MAX_GAS_LIMIT", defaults.max_gas_limit)?,
			max_gas_price_wei_bep20: gwei_to_wei(env_cap(lookup, "SIGNER_MAX_GAS_PRICE_GWEI_BEP20", DEFAULT_MAX_GAS_PRICE_GWEI_BEP20)?),
			max_gas_price_wei_polygon: gwei_to_wei(env_cap(lookup, "SIGNER_MAX_GAS_PRICE_GWEI_POLYGON", DEFAULT_MAX_GAS_PRICE_GWEI_POLYGON)?),
			max_tron_fee_limit_sun: env_cap(lookup, "SIGNER_MAX_TRON_FEE_LIMIT_SUN", defaults.max_tron_fee_limit_sun)?,
			max_ton_msg_value_nano: env_cap(lookup, "SIGNER_MAX_TON_MSG_VALUE_NANO", defaults.max_ton_msg_value_nano)?,
			max_ton_forward_nano: env_cap(lookup, "SIGNER_MAX_TON_FORWARD_NANO", defaults.max_ton_forward_nano)?,
		})
	}

	/// Enforce the budget on the fee side of a transaction about to be signed on `network`.
	/// Every breach is `permission_denied` naming the field, the value and the ceiling — a
	/// policy refusal the hub parks the withdrawal on, not a malformed request.
	pub fn check(&self, network: Network, quote: FeeQuote) -> Result<(), Status> {
		match quote {
			FeeQuote::Evm { gas_price, gas_limit } => {
				// The fee is the product; a quote whose product does not even fit is refused
				// outright rather than compared after wrapping.
				gas_price
					.checked_mul(u128::from(gas_limit))
					.ok_or_else(|| Status::permission_denied(format!("gas_price {gas_price} × gas_limit {gas_limit} overflows the signer's fee budget on {network}")))?;
				deny_over(network, "gas_limit", u128::from(gas_limit), u128::from(self.max_gas_limit))?;
				deny_over(network, "gas_price", gas_price, self.max_gas_price_wei(network)?)
			}
			FeeQuote::Tron { fee_limit } => {
				let fee_limit = u128::try_from(fee_limit).map_err(|_| Status::invalid_argument("fee_limit must not be negative"))?;
				deny_over(network, "fee_limit", fee_limit, u128::from(self.max_tron_fee_limit_sun))
			}
			FeeQuote::Ton { msg_value, forward_ton_amount } => {
				deny_over(network, "msg_value", u128::from(msg_value), u128::from(self.max_ton_msg_value_nano))?;
				deny_over(network, "forward_ton_amount", u128::from(forward_ton_amount), u128::from(self.max_ton_forward_nano))?;
				if forward_ton_amount > msg_value {
					return Err(Status::permission_denied(format!(
						"forward_ton_amount {forward_ton_amount} exceeds msg_value {msg_value} it is paid out of on {network}"
					)));
				}
				Ok(())
			}
		}
	}

	fn max_gas_price_wei(&self, network: Network) -> Result<u128, Status> {
		match network {
			Network::Bep20 => Ok(self.max_gas_price_wei_bep20),
			Network::Polygon => Ok(self.max_gas_price_wei_polygon),
			// The handlers pin the rail before quoting a fee, so this is our bug, not the caller's.
			Network::Trc20 | Network::Ton => Err(Status::internal(format!("an EVM fee quote was checked against {network}"))),
		}
	}
}

/// Lossless: `u64::MAX` gwei is ~1.8e28 wei, well inside `u128`.
const fn gwei_to_wei(gwei: u64) -> u128 {
	gwei as u128 * WEI_PER_GWEI
}

/// One ceiling from `lookup`: unset/empty ⇒ `default`; `0`, negative or unparsable ⇒ error.
fn env_cap(lookup: &impl Fn(&str) -> Option<String>, name: &str, default: u64) -> color_eyre::Result<u64> {
	match lookup(name).filter(|raw| !raw.is_empty()) {
		Some(raw) => {
			let value = raw
				.trim()
				.parse::<u64>()
				.map_err(|_| color_eyre::eyre::eyre!("{name} must be a positive whole number, got {raw:?}"))?;
			if value == 0 {
				return Err(color_eyre::eyre::eyre!("{name} must be positive — the fee budget cannot be disabled"));
			}
			Ok(value)
		}
		None => Ok(default),
	}
}

fn deny_over(network: Network, field: &str, value: u128, cap: u128) -> Result<(), Status> {
	if value > cap {
		return Err(Status::permission_denied(format!("{field} {value} exceeds the signer's fee budget cap of {cap} on {network}")));
	}
	Ok(())
}

impl SignerPolicy {
	pub fn from_env() -> color_eyre::Result<Self> {
		Self::from_lookup(&|name| std::env::var(name).ok())
	}

	/// Build the policy from `lookup` (the environment in production; a map in tests, which
	/// must not mutate the process environment under a parallel test runner).
	pub fn from_lookup(lookup: &impl Fn(&str) -> Option<String>) -> color_eyre::Result<Self> {
		let max_transfer_usdt = match lookup("SIGNER_MAX_TRANSFER_USDT").filter(|s| !s.is_empty()) {
			Some(raw) => Some(
				raw.parse::<u64>()
					.map_err(|_| color_eyre::eyre::eyre!("SIGNER_MAX_TRANSFER_USDT must be a whole number of USDT"))?,
			),
			None => None,
		};
		let destination_allowlist = lookup("SIGNER_DESTINATION_ALLOWLIST")
			.map(|raw| raw.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect())
			.unwrap_or_default();
		let ton_treasury_jetton_wallet = match lookup("SIGNER_TON_TREASURY_JETTON_WALLET").map(|raw| raw.trim().to_owned()).filter(|s| !s.is_empty()) {
			// A pin that does not parse would refuse every TON withdrawal; fail the boot instead.
			Some(raw) if tonlib_core::TonAddress::from_str(&raw).is_ok() => Some(raw),
			Some(raw) => return Err(color_eyre::eyre::eyre!("SIGNER_TON_TREASURY_JETTON_WALLET is not a TON address: {raw:?}")),
			None => None,
		};
		let fee_budget = FeeBudget::from_lookup(lookup)?;
		Ok(Self {
			max_transfer_usdt,
			destination_allowlist,
			ton_treasury_jetton_wallet,
			fee_budget,
		})
	}

	/// Whether any opt-in control (cap, allowlist, pinned jetton wallet) is active, for a
	/// one-line boot log. The fee budget is always on and is not part of this answer.
	pub fn is_active(&self) -> bool {
		self.max_transfer_usdt.is_some() || !self.destination_allowlist.is_empty() || self.ton_treasury_jetton_wallet.is_some()
	}

	pub fn treasury_jetton_wallet_pinned(&self) -> bool {
		self.ton_treasury_jetton_wallet.is_some()
	}

	pub fn max_transfer_usdt(&self) -> Option<u64> {
		self.max_transfer_usdt
	}

	pub fn allowlist_len(&self) -> usize {
		self.destination_allowlist.len()
	}

	pub fn fee_budget(&self) -> &FeeBudget {
		&self.fee_budget
	}

	/// Enforce the fee budget on a transaction about to be signed — from ANY wallet.
	pub fn check_fee_budget(&self, network: Network, quote: FeeQuote) -> Result<(), Status> {
		self.fee_budget.check(network, quote)
	}

	/// Enforce the policy on a treasury-sourced USDT transfer. `amount_base_units` is the
	/// transfer amount in `network`'s on-chain decimals (as it will be signed), so the cap is
	/// compared like-for-like after lowering the whole-USDT limit to the chain's precision. A
	/// breach is `permission_denied` — a policy refusal, not a malformed request.
	pub fn check_treasury_transfer(&self, network: Network, to_address: &str, amount_base_units: u128) -> Result<(), Status> {
		if let Some(cap_usdt) = self.max_transfer_usdt {
			let cap = Usdt::from_base_units(u128::from(cap_usdt).saturating_mul(CANONICAL_PER_USDT))
				.to_onchain(network)
				.map_err(|_| Status::internal("signer cap is not representable on this network"))?;
			if amount_base_units > cap {
				return Err(Status::permission_denied(format!(
					"treasury transfer of {amount_base_units} exceeds the signer's per-transfer cap of {cap_usdt} USDT ({cap} on {network})"
				)));
			}
		}
		self.check_allowlist(to_address)
	}

	/// Enforce the policy on a treasury-sourced NATIVE (gas-coin) transfer: only the
	/// destination allowlist applies — the per-transfer cap is USDT-denominated and cannot
	/// price a native amount. No core flow sends native funds FROM the treasury today (gas
	/// top-ups are signed from the gas-station wallet), so an operator enabling the
	/// allowlist must include any deliberate treasury-native destination on it.
	pub fn check_treasury_native_transfer(&self, to_address: &str) -> Result<(), Status> {
		self.check_allowlist(to_address)
	}

	/// Enforce the policy on a treasury jetton transfer's `response_destination` — the address
	/// the excess Toncoin returns to, and so a second destination on the same signed message.
	/// Unlike `to_address` this is never a user's address: a legitimate withdrawal always points
	/// it at the treasury itself, so the check is ALWAYS on — the sending wallet's own address
	/// is accepted in either rendering (the raw `0:<hex>` the signer stores, the base64 the hub
	/// may carry), and anything else only if an allowlist is set and lists it verbatim.
	pub fn check_treasury_response_destination(&self, own_address: &str, response_destination: &str) -> Result<(), Status> {
		if provision::addresses_agree(Network::Ton, own_address, response_destination) || self.destination_allowlist.contains(response_destination) {
			return Ok(());
		}
		Err(Status::permission_denied(
			"treasury transfer response_destination must be the sending wallet or on the signer's allowlist",
		))
	}

	/// Enforce the policy on a treasury jetton transfer's `our_jetton_wallet` — the internal
	/// message's destination, which receives `msg_value` in native Toncoin whatever contract
	/// sits there. Pinned ⇒ it must be the pinned address (either rendering); unpinned ⇒ it is
	/// held to the destination allowlist like `to_address` (a no-op while the list is empty).
	pub fn check_treasury_jetton_wallet(&self, our_jetton_wallet: &str) -> Result<(), Status> {
		match &self.ton_treasury_jetton_wallet {
			Some(pinned) if provision::addresses_agree(Network::Ton, pinned, our_jetton_wallet) => Ok(()),
			Some(_) => Err(Status::permission_denied("our_jetton_wallet is not the treasury's pinned jetton wallet")),
			None => self
				.check_allowlist(our_jetton_wallet)
				.map_err(|_| Status::permission_denied("our_jetton_wallet is neither pinned nor on the signer's allowlist")),
		}
	}

	/// Verbatim membership on purpose: a rendering-aware comparison (EIP-55 casing, TON raw vs
	/// base64) is #183's change, not this one.
	fn check_allowlist(&self, to_address: &str) -> Result<(), Status> {
		if !self.destination_allowlist.is_empty() && !self.destination_allowlist.contains(to_address) {
			return Err(Status::permission_denied("treasury transfer destination is not on the signer's allowlist"));
		}
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use std::collections::HashMap;

	use tonic::Code;

	use super::*;

	fn policy(max: Option<u64>, allow: &[&str]) -> SignerPolicy {
		SignerPolicy {
			max_transfer_usdt: max,
			destination_allowlist: allow.iter().map(|s| (*s).to_owned()).collect(),
			ton_treasury_jetton_wallet: None,
			fee_budget: FeeBudget::default(),
		}
	}

	fn denied(result: Result<(), Status>) -> Status {
		let status = result.expect_err("expected a refusal");
		assert_eq!(status.code(), Code::PermissionDenied, "{status:?}");
		status
	}

	#[test]
	fn unconfigured_policy_allows_everything() {
		let p = SignerPolicy::default();
		assert!(!p.is_active());
		// 1e30 base units, any address — no cap, no allowlist ⇒ allowed.
		assert!(p.check_treasury_transfer(Network::Bep20, "0xanything", 1_000_000_000_000_000_000_000_000_000_000).is_ok());
	}

	#[test]
	fn cap_is_scaled_to_each_chain_precision() {
		let p = policy(Some(1000), &[]);
		// BEP20 USDT is 18-dp: 1000 USDT = 1000e18 base units.
		let cap_bep20 = 1000u128 * CANONICAL_PER_USDT;
		assert!(p.check_treasury_transfer(Network::Bep20, "0xto", cap_bep20).is_ok());
		assert!(p.check_treasury_transfer(Network::Bep20, "0xto", cap_bep20 + 1).is_err());
		// TRC20/TON USDT is 6-dp: 1000 USDT = 1_000_000_000 base units.
		assert!(p.check_treasury_transfer(Network::Trc20, "Tto", 1_000_000_000).is_ok());
		assert!(p.check_treasury_transfer(Network::Trc20, "Tto", 1_000_000_001).is_err());
		assert!(p.check_treasury_transfer(Network::Ton, "EQto", 1_000_000_000).is_ok());
	}

	#[test]
	fn allowlist_pins_destinations_when_set() {
		let p = policy(None, &["0xgood", "0xalsogood"]);
		assert!(p.check_treasury_transfer(Network::Bep20, "0xgood", 1).is_ok());
		assert!(p.check_treasury_transfer(Network::Bep20, "0xbad", 1).is_err());
	}

	#[test]
	fn native_transfers_honor_the_allowlist_but_not_the_usdt_cap() {
		let p = policy(Some(1), &["0xgood"]);
		// On the allowlist → allowed regardless of the (inapplicable) USDT cap.
		assert!(p.check_treasury_native_transfer("0xgood").is_ok());
		assert!(p.check_treasury_native_transfer("0xbad").is_err());
		// Allowlist unset → no-op, even with a cap configured.
		let p = policy(Some(1), &[]);
		assert!(p.check_treasury_native_transfer("0xanything").is_ok());
	}

	#[test]
	fn cap_and_allowlist_compose() {
		let p = policy(Some(1000), &["0xgood"]);
		// On the allowlist but over the cap → denied.
		assert!(p.check_treasury_transfer(Network::Bep20, "0xgood", 2000 * CANONICAL_PER_USDT).is_err());
		// Under the cap but off the allowlist → denied.
		assert!(p.check_treasury_transfer(Network::Bep20, "0xother", 1).is_err());
		// Under the cap and on the allowlist → allowed.
		assert!(p.check_treasury_transfer(Network::Bep20, "0xgood", 500 * CANONICAL_PER_USDT).is_ok());
	}

	// === fee budget =============================================================

	const GWEI: u128 = WEI_PER_GWEI;

	#[test]
	fn evm_budget_accepts_at_cap_and_refuses_one_over() {
		let p = SignerPolicy::default();
		// BSC: 100 gwei × 100_000 gas is the whole default budget.
		assert!(
			p.check_fee_budget(
				Network::Bep20,
				FeeQuote::Evm {
					gas_price: 100 * GWEI,
					gas_limit: 100_000
				}
			)
			.is_ok()
		);
		let over_price = denied(p.check_fee_budget(
			Network::Bep20,
			FeeQuote::Evm {
				gas_price: 100 * GWEI + 1,
				gas_limit: 100_000,
			},
		));
		assert!(over_price.message().contains("gas_price"), "{over_price:?}");
		let over_limit = denied(p.check_fee_budget(Network::Bep20, FeeQuote::Evm { gas_price: 1, gas_limit: 100_001 }));
		assert!(over_limit.message().contains("gas_limit"), "{over_limit:?}");
		// Polygon has its own, higher price ceiling: what BSC refuses, Polygon accepts.
		assert!(
			p.check_fee_budget(
				Network::Polygon,
				FeeQuote::Evm {
					gas_price: 5_000 * GWEI,
					gas_limit: 100_000
				}
			)
			.is_ok()
		);
		denied(p.check_fee_budget(
			Network::Polygon,
			FeeQuote::Evm {
				gas_price: 5_000 * GWEI + 1,
				gas_limit: 100_000,
			},
		));
	}

	#[test]
	fn evm_budget_refuses_an_overflowing_product_rather_than_wrapping_it() {
		let p = SignerPolicy::default();
		// u128::MAX × 2 wraps to u128::MAX - 1 — under no sane cap, but the product is what is
		// burned, so it must be refused as such rather than reasoned about after wrapping.
		let status = denied(p.check_fee_budget(Network::Bep20, FeeQuote::Evm { gas_price: u128::MAX, gas_limit: 2 }));
		assert!(status.message().contains("overflows"), "{status:?}");
		// The other rails never see an EVM quote; if one does, it is our bug, not a policy verdict.
		assert_eq!(p.check_fee_budget(Network::Ton, FeeQuote::Evm { gas_price: 1, gas_limit: 1 }).unwrap_err().code(), Code::Internal);
	}

	#[test]
	fn tron_budget_bounds_fee_limit_and_keeps_negative_as_malformed() {
		let p = SignerPolicy::default();
		assert!(p.check_fee_budget(Network::Trc20, FeeQuote::Tron { fee_limit: 100_000_000 }).is_ok());
		denied(p.check_fee_budget(Network::Trc20, FeeQuote::Tron { fee_limit: 100_000_001 }));
		assert_eq!(p.check_fee_budget(Network::Trc20, FeeQuote::Tron { fee_limit: -1 }).unwrap_err().code(), Code::InvalidArgument);
	}

	#[test]
	fn ton_budget_bounds_both_amounts_and_their_order() {
		let p = SignerPolicy::default();
		assert!(
			p.check_fee_budget(
				Network::Ton,
				FeeQuote::Ton {
					msg_value: 100_000_000,
					forward_ton_amount: 50_000_000
				}
			)
			.is_ok()
		);
		let over_msg = denied(p.check_fee_budget(
			Network::Ton,
			FeeQuote::Ton {
				msg_value: 100_000_001,
				forward_ton_amount: 1,
			},
		));
		assert!(over_msg.message().contains("msg_value"), "{over_msg:?}");
		let over_fwd = denied(p.check_fee_budget(
			Network::Ton,
			FeeQuote::Ton {
				msg_value: 100_000_000,
				forward_ton_amount: 50_000_001,
			},
		));
		assert!(over_fwd.message().contains("forward_ton_amount"), "{over_fwd:?}");
		// Both under their caps, but the forward amount cannot exceed the value it is paid from.
		let inverted = denied(p.check_fee_budget(
			Network::Ton,
			FeeQuote::Ton {
				msg_value: 10_000_000,
				forward_ton_amount: 20_000_000,
			},
		));
		assert!(inverted.message().contains("exceeds msg_value"), "{inverted:?}");
		// Forwarding the whole value is the boundary, and it is allowed.
		assert!(
			p.check_fee_budget(
				Network::Ton,
				FeeQuote::Ton {
					msg_value: 50_000_000,
					forward_ton_amount: 50_000_000,
				},
			)
			.is_ok()
		);
	}

	// === fee budget: env parsing ===============================================

	fn lookup(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
		let vars: HashMap<String, String> = vars.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
		move |name| vars.get(name).cloned()
	}

	#[test]
	fn fee_budget_defaults_when_env_is_unset_or_empty() {
		assert_eq!(FeeBudget::from_lookup(&lookup(&[])).unwrap(), FeeBudget::default());
		assert_eq!(FeeBudget::from_lookup(&lookup(&[("SIGNER_MAX_GAS_LIMIT", "")])).unwrap(), FeeBudget::default());
	}

	#[test]
	fn fee_budget_trims_a_padded_value_but_not_to_nothing() {
		let padded = FeeBudget::from_lookup(&lookup(&[("SIGNER_MAX_GAS_LIMIT", " 100 ")])).unwrap();
		assert_eq!(padded.max_gas_limit, 100);
		// Whitespace alone is set-but-unparsable, not unset: it must not fall back to the default.
		assert!(FeeBudget::from_lookup(&lookup(&[("SIGNER_MAX_GAS_LIMIT", " ")])).is_err());
	}

	#[test]
	fn fee_budget_env_overrides_apply() {
		let budget = FeeBudget::from_lookup(&lookup(&[
			("SIGNER_MAX_GAS_LIMIT", "200000"),
			("SIGNER_MAX_GAS_PRICE_GWEI_BEP20", "250"),
			("SIGNER_MAX_GAS_PRICE_GWEI_POLYGON", "10000"),
			("SIGNER_MAX_TRON_FEE_LIMIT_SUN", "150000000"),
			("SIGNER_MAX_TON_MSG_VALUE_NANO", "200000000"),
			("SIGNER_MAX_TON_FORWARD_NANO", "60000000"),
		]))
		.unwrap();
		assert_eq!(
			budget,
			FeeBudget {
				max_gas_limit: 200_000,
				max_gas_price_wei_bep20: 250 * GWEI,
				max_gas_price_wei_polygon: 10_000 * GWEI,
				max_tron_fee_limit_sun: 150_000_000,
				max_ton_msg_value_nano: 200_000_000,
				max_ton_forward_nano: 60_000_000,
			}
		);
		// A raised ceiling is honored where the default would have refused.
		let p = SignerPolicy {
			fee_budget: budget,
			..SignerPolicy::default()
		};
		assert!(
			p.check_fee_budget(
				Network::Bep20,
				FeeQuote::Evm {
					gas_price: 250 * GWEI,
					gas_limit: 200_000
				}
			)
			.is_ok()
		);
		denied(p.check_fee_budget(
			Network::Bep20,
			FeeQuote::Evm {
				gas_price: 251 * GWEI,
				gas_limit: 200_000,
			},
		));
	}

	#[test]
	fn fee_budget_refuses_zero_and_garbage_at_boot() {
		for name in [
			"SIGNER_MAX_GAS_LIMIT",
			"SIGNER_MAX_GAS_PRICE_GWEI_BEP20",
			"SIGNER_MAX_GAS_PRICE_GWEI_POLYGON",
			"SIGNER_MAX_TRON_FEE_LIMIT_SUN",
			"SIGNER_MAX_TON_MSG_VALUE_NANO",
			"SIGNER_MAX_TON_FORWARD_NANO",
		] {
			for bad in ["0", "abc", "-5", "1.5"] {
				let err = FeeBudget::from_lookup(&lookup(&[(name, bad)])).expect_err(&format!("{name}={bad} must not boot"));
				assert!(err.to_string().contains(name), "{err}");
			}
		}
	}

	// === response_destination ==================================================

	// A real v4R2 wallet in both renderings (the vector `key_vault` pins): the raw form the
	// signer stores for its own address, and the user-friendly base64 the hub may send back.
	const OWN_BASE64: &str = "UQCS65EGyiApUTLOYXDs4jOLoQNCE0o8oNnkmfIcm0iX5FRT";
	const FOREIGN: &str = "EQB3ncyBUTjZUA5EnFKR5_EnOMI9V1tTEAAPaiU71gc4TiUt";

	fn own_raw() -> String {
		tonlib_core::TonAddress::from_str(OWN_BASE64).unwrap().to_hex()
	}

	#[test]
	fn response_destination_accepts_the_wallet_itself_in_either_rendering() {
		let p = policy(None, &["EQsomeone_else"]);
		let own = own_raw();
		assert!(own.starts_with("0:"));
		// Spelled differently from the stored raw form, and not on the allowlist — still the
		// wallet's own address.
		assert!(p.check_treasury_response_destination(&own, OWN_BASE64).is_ok());
		assert!(p.check_treasury_response_destination(&own, &own).is_ok());
	}

	#[test]
	fn response_destination_refuses_a_foreign_address_unless_allowlisted() {
		let own = own_raw();
		let status = denied(policy(None, &["EQsomeone_else"]).check_treasury_response_destination(&own, FOREIGN));
		assert!(status.message().contains("response_destination"), "{status:?}");
		assert!(policy(None, &[FOREIGN]).check_treasury_response_destination(&own, FOREIGN).is_ok());
		// Garbage is not the wallet's own address either.
		denied(policy(None, &[FOREIGN]).check_treasury_response_destination(&own, "not-an-address"));
	}

	#[test]
	fn response_destination_must_be_the_own_wallet_without_an_allowlist() {
		// The default (prod) posture: no allowlist, yet the excess may only return to the wallet.
		let p = policy(Some(1000), &[]);
		assert!(p.check_treasury_response_destination(&own_raw(), OWN_BASE64).is_ok());
		denied(p.check_treasury_response_destination(&own_raw(), FOREIGN));
		denied(p.check_treasury_response_destination(&own_raw(), "anything"));
	}

	// === our_jetton_wallet =====================================================

	const JETTON_WALLET_RAW: &str = "0:e4d954ef9f4e1250a26b5bbad76a1cdd17cfd08babad6f4c23e372270aef6f76";

	fn jetton_wallet_base64() -> String {
		tonlib_core::TonAddress::from_str(JETTON_WALLET_RAW).unwrap().to_base64_url_flags(true, false)
	}

	#[test]
	fn pinned_jetton_wallet_admits_only_itself_in_either_rendering() {
		let p = SignerPolicy {
			ton_treasury_jetton_wallet: Some(JETTON_WALLET_RAW.to_owned()),
			..policy(None, &[FOREIGN])
		};
		assert!(p.is_active());
		assert!(p.check_treasury_jetton_wallet(JETTON_WALLET_RAW).is_ok());
		assert!(p.check_treasury_jetton_wallet(&jetton_wallet_base64()).is_ok());
		// Allowlisted, but not the pin: the pin wins.
		let status = denied(p.check_treasury_jetton_wallet(FOREIGN));
		assert!(status.message().contains("pinned"), "{status:?}");
		denied(p.check_treasury_jetton_wallet("garbage"));
	}

	#[test]
	fn unpinned_jetton_wallet_falls_back_to_the_allowlist() {
		// No list, no pin: the default posture, unchecked (as `to_address` is).
		assert!(policy(None, &[]).check_treasury_jetton_wallet(FOREIGN).is_ok());
		// A list without the jetton wallet on it refuses — the operator must list or pin it.
		let status = denied(policy(None, &[FOREIGN]).check_treasury_jetton_wallet(JETTON_WALLET_RAW));
		assert!(status.message().contains("our_jetton_wallet"), "{status:?}");
		assert!(policy(None, &[JETTON_WALLET_RAW]).check_treasury_jetton_wallet(JETTON_WALLET_RAW).is_ok());
	}

	// === policy: env parsing ===================================================

	#[test]
	fn policy_from_lookup_reads_cap_allowlist_and_pin() {
		let p = SignerPolicy::from_lookup(&lookup(&[])).unwrap();
		assert!(!p.is_active());
		assert!(!p.treasury_jetton_wallet_pinned());

		let p = SignerPolicy::from_lookup(&lookup(&[
			("SIGNER_MAX_TRANSFER_USDT", "500"),
			("SIGNER_DESTINATION_ALLOWLIST", " 0xa , 0xb,, "),
			("SIGNER_TON_TREASURY_JETTON_WALLET", JETTON_WALLET_RAW),
		]))
		.unwrap();
		assert_eq!(p.max_transfer_usdt(), Some(500));
		assert_eq!(p.allowlist_len(), 2);
		assert!(p.treasury_jetton_wallet_pinned());
		assert!(p.check_treasury_jetton_wallet(&jetton_wallet_base64()).is_ok());

		// An empty pin is unset; a pin that is not a TON address does not boot.
		assert!(
			!SignerPolicy::from_lookup(&lookup(&[("SIGNER_TON_TREASURY_JETTON_WALLET", "")]))
				.unwrap()
				.treasury_jetton_wallet_pinned()
		);
		let err = SignerPolicy::from_lookup(&lookup(&[("SIGNER_TON_TREASURY_JETTON_WALLET", "not-an-address")])).unwrap_err();
		assert!(err.to_string().contains("SIGNER_TON_TREASURY_JETTON_WALLET"), "{err}");
		assert!(SignerPolicy::from_lookup(&lookup(&[("SIGNER_MAX_TRANSFER_USDT", "x")])).is_err());
	}
}
