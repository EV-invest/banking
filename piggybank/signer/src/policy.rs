//! The signer's independent spend policy — the second gate that holds even if the hub is
//! compromised.
//!
//! The signer is a distinct trust domain: it holds the keys and applies its OWN limits, so
//! an attacker who owns the hub still cannot make it sign an arbitrary payout. Today the two
//! controls that matter most before scaling real liquidity:
//!
//!   - a **per-transfer USDT cap** — a single signed treasury transfer can move at most this
//!     much, so one forged request can't drain the hot wallet;
//!   - an optional **destination allowlist** — when set, treasury transfers may only go to
//!     pre-registered addresses (a hardened/staged posture; off by default, since the normal
//!     withdrawal model sends to arbitrary user addresses).
//!
//! Both apply only to transfers signed FROM the **treasury** wallet (the withdrawal drain
//! vector); sweeps *into* the treasury and gas top-ups (signed from the separate gas-station
//! wallet) are not treasury spends. Native (gas-coin) transfers from the treasury honor the
//! allowlist too, but not the cap — it is USDT-denominated and cannot price a native amount.
//! Both controls are **no-ops until configured** (`SIGNER_MAX_TRANSFER_USDT`,
//! `SIGNER_DESTINATION_ALLOWLIST`), so dev/CI and existing deployments are unaffected until
//! an operator opts in — the same convention as the observability seams.
//!
//! `Status` is tonic's large error type we don't control (same as the service handlers).
#![allow(clippy::result_large_err)]

use std::collections::HashSet;

use domain::money::{Network, Usdt};
use tonic::Status;

/// Canonical base units per whole USDT (the domain's 18-dp representation).
const CANONICAL_PER_USDT: u128 = 1_000_000_000_000_000_000;

/// The signer's spend policy, loaded once at boot and consulted on every treasury transfer.
#[derive(Clone, Debug, Default)]
pub struct SignerPolicy {
	/// Max USDT (whole units) a single treasury transfer may move. `None` ⇒ uncapped.
	max_transfer_usdt: Option<u64>,
	/// If non-empty, a treasury transfer's destination must be one of these (verbatim wire
	/// address strings). Empty ⇒ any destination is allowed (the default withdrawal model).
	destination_allowlist: HashSet<String>,
}

impl SignerPolicy {
	pub fn from_env() -> anyhow::Result<Self> {
		let max_transfer_usdt = match std::env::var("SIGNER_MAX_TRANSFER_USDT").ok().filter(|s| !s.is_empty()) {
			Some(raw) => Some(raw.parse::<u64>().map_err(|_| anyhow::anyhow!("SIGNER_MAX_TRANSFER_USDT must be a whole number of USDT"))?),
			None => None,
		};
		let destination_allowlist = std::env::var("SIGNER_DESTINATION_ALLOWLIST")
			.ok()
			.map(|raw| raw.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect())
			.unwrap_or_default();
		Ok(Self {
			max_transfer_usdt,
			destination_allowlist,
		})
	}

	/// Whether any control is active (for a one-line boot log).
	pub fn is_active(&self) -> bool {
		self.max_transfer_usdt.is_some() || !self.destination_allowlist.is_empty()
	}

	pub fn max_transfer_usdt(&self) -> Option<u64> {
		self.max_transfer_usdt
	}

	pub fn allowlist_len(&self) -> usize {
		self.destination_allowlist.len()
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

	fn check_allowlist(&self, to_address: &str) -> Result<(), Status> {
		if !self.destination_allowlist.is_empty() && !self.destination_allowlist.contains(to_address) {
			return Err(Status::permission_denied("treasury transfer destination is not on the signer's allowlist"));
		}
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn policy(max: Option<u64>, allow: &[&str]) -> SignerPolicy {
		SignerPolicy {
			max_transfer_usdt: max,
			destination_allowlist: allow.iter().map(|s| (*s).to_owned()).collect(),
		}
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
}
