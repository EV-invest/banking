//! The signer's independent spend policy — the second gate that holds even if the hub is
//! compromised.
//!
//! The signer is a distinct trust domain: it holds the keys and applies its OWN limits, so
//! an attacker who owns the hub still cannot make it sign an arbitrary payout. Every wallet
//! the signer holds a key for belongs to one of three classes, and each class has the rule
//! that fits it — there is no wallet a handler signs for without reaching a policy branch:
//!
//!   - **Deposit** (a user's address; anything but the two reserved ids): the only legitimate
//!     flow is a **sweep into the treasury**, so a token transfer's destination must be the
//!     treasury's own address on that network — which the signer already knows from its own
//!     `wallet_secrets`, so no configuration is involved and no legitimate sweep is refused.
//!     A treasury that is not provisioned on the network means there is nowhere to sweep to,
//!     and the request is refused. A native transfer out of a deposit wallet has no
//!     legitimate flow at all and is always refused. On TON the hub points a sweep's
//!     `response_destination` at the gas station (the excess Toncoin tops the station back
//!     up), so that field may name the sending wallet, the gas station or the treasury — all
//!     three derived by the signer. `our_jetton_wallet` cannot be derived for a sweep (it is
//!     the user's jetton wallet, a contract address the signer never computes); it is
//!     bounded instead by the fee budget on `msg_value` — the only Toncoin that message
//!     carries — and by the native spend window (#369).
//!   - **Gas station** (`Uuid::from_u128(1)`, the hub's reserved id): it only ever tops up
//!     deposit wallets with the native coin, so it signs **native transfers only**, the
//!     destination must be an address the signer itself holds a key for on that network
//!     (again derived from `wallet_secrets`, not configured), and the drip is capped by
//!     [`GasTopupCaps`].
//!   - **Treasury** (`Uuid::nil()`): the one class with an arbitrary destination — user
//!     withdrawals — and so the one where the amount controls live:
//!       - a **per-transfer USDT cap** (`SIGNER_MAX_TRANSFER_USDT`) — a single signed
//!         treasury transfer can move at most this much, so one forged request can't drain
//!         the hot wallet;
//!       - an optional **destination allowlist** (`SIGNER_DESTINATION_ALLOWLIST`) — when set,
//!         treasury transfers may only go to pre-registered addresses (a hardened/staged
//!         posture; off by default, since the normal withdrawal model sends to arbitrary user
//!         addresses). Membership is rendering-aware ([`provision::addresses_agree`]): EIP-55
//!         casing and TON's raw-vs-base64 forms are the same address, so an operator may list
//!         either spelling. A TON jetton transfer names two more addresses that receive native
//!         Toncoin, and both are held to the same list: the `response_destination` (the excess
//!         returns there — always the sending wallet itself on a legitimate withdrawal, so that
//!         is accepted with or without a list) and `our_jetton_wallet` (the internal message's
//!         destination, which receives `msg_value` — pinned with
//!         `SIGNER_TON_TREASURY_JETTON_WALLET`, or else it must be on the list).
//!
//! Native (gas-coin) transfers from the treasury honor the allowlist but not the cap — it is
//! USDT-denominated and cannot price a native amount. The cap and the allowlist are
//! **no-ops until configured**, so dev/CI and existing deployments are unaffected until an
//! operator opts in — the same convention as the observability seams. An operator enabling
//! the allowlist must either pin the treasury's jetton wallet or put it on the list, or every
//! TON withdrawal is refused.
//!
//! Two controls are the opposite — **on by default** with ceilings an honest hub never
//! reaches, and an operator may raise them via their `SIGNER_MAX_*` variables but cannot
//! switch them off (`0` is a boot error, not "disabled"):
//!
//!   - the **fee budget** ([`FeeBudget`]) — a ceiling on the gas/fee side of EVERY signed
//!     transaction, on every wallet class. The amount caps above bound what leaves in USDT;
//!     they say nothing about the native coin a transaction burns as fee. Without this gate a
//!     forged request moving 1 USDT with an absurd `gas_price` would hand the whole native
//!     balance to the miner, and could do so once per nonce;
//!   - the **gas top-up cap** ([`GasTopupCaps`]) — a ceiling on the native amount one
//!     gas-station drip may carry.
//!
//! `Status` is tonic's large error type we don't control (same as the service handlers).
#![allow(clippy::result_large_err)]

use std::str::FromStr as _;

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
	/// If non-empty, a treasury transfer's destination must agree with one of these (wire
	/// address strings in any rendering — membership goes through
	/// [`provision::addresses_agree`], not string equality). Empty ⇒ any destination is
	/// allowed (the default withdrawal model).
	destination_allowlist: Vec<String>,
	/// The treasury's own USDT jetton wallet, when pinned: a treasury jetton transfer's
	/// `our_jetton_wallet` must be this address. `None` ⇒ it falls back to the allowlist.
	ton_treasury_jetton_wallet: Option<String>,
	/// The always-on ceiling on the fee side of every signed transaction.
	fee_budget: FeeBudget,
	/// The always-on ceiling on the native amount one gas-station drip may carry.
	gas_topup: GasTopupCaps,
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

/// Ceilings on the native amount one gas-station drip may carry, per rail, in the rail's
/// base units (wei / wei / SUN / nanoton).
///
/// Sized from what an honest hub sends at the signer's OWN fee-budget ceilings, with headroom
/// so the two gates never disagree on a legitimate top-up; an operator may raise one via
/// `SIGNER_MAX_GAS_TOPUP_{BEP20,POLYGON,TRC20,TON}`:
///   - EVM: a drip is `max(gas_price × gas_limit × gas_drop_multiple, min_gas_drop_wei)` —
///     `SweepConfig` defaults 3 and 3e14 — so at the fee budget's ceilings (100 gwei /
///     5_000 gwei × 100_000 gas) the largest honest drip is 0.03 BNB / 1.5 POL. Caps:
///     BEP20 5e16 wei (0.05 BNB), Polygon 2e18 wei (2 POL).
///   - Tron: a drip is the flat `TRON_SWEEP_MIN_TRX_DROP_SUN`, 30 TRX. Cap 5e7 SUN (50 TRX).
///   - TON: a drip is `max(TON_SWEEP_GAS_TOPUP_NANO, msg_value + 0.05 TON headroom)` — 0.15
///     TON at the fee budget's `msg_value` ceiling. Cap 2e8 nanoton (0.2 TON).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GasTopupCaps {
	bep20_wei: u128,
	polygon_wei: u128,
	trc20_sun: u128,
	ton_nano: u128,
}

impl Default for GasTopupCaps {
	fn default() -> Self {
		Self {
			bep20_wei: 50_000_000_000_000_000,
			polygon_wei: 2_000_000_000_000_000_000,
			trc20_sun: 50_000_000,
			ton_nano: 200_000_000,
		}
	}
}

impl GasTopupCaps {
	fn from_lookup(lookup: &impl Fn(&str) -> Option<String>) -> color_eyre::Result<Self> {
		let defaults = Self::default();
		Ok(Self {
			bep20_wei: env_cap(lookup, "SIGNER_MAX_GAS_TOPUP_BEP20", defaults.bep20_wei)?,
			polygon_wei: env_cap(lookup, "SIGNER_MAX_GAS_TOPUP_POLYGON", defaults.polygon_wei)?,
			trc20_sun: env_cap(lookup, "SIGNER_MAX_GAS_TOPUP_TRC20", defaults.trc20_sun)?,
			ton_nano: env_cap(lookup, "SIGNER_MAX_GAS_TOPUP_TON", defaults.ton_nano)?,
		})
	}

	fn cap(&self, network: Network) -> u128 {
		match network {
			Network::Bep20 => self.bep20_wei,
			Network::Polygon => self.polygon_wei,
			Network::Trc20 => self.trc20_sun,
			Network::Ton => self.ton_nano,
		}
	}
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
				deny_over(network, FEE_BUDGET, "gas_limit", u128::from(gas_limit), u128::from(self.max_gas_limit))?;
				deny_over(network, FEE_BUDGET, "gas_price", gas_price, self.max_gas_price_wei(network)?)
			}
			FeeQuote::Tron { fee_limit } => {
				let fee_limit = u128::try_from(fee_limit).map_err(|_| Status::invalid_argument("fee_limit must not be negative"))?;
				deny_over(network, FEE_BUDGET, "fee_limit", fee_limit, u128::from(self.max_tron_fee_limit_sun))
			}
			FeeQuote::Ton { msg_value, forward_ton_amount } => {
				deny_over(network, FEE_BUDGET, "msg_value", u128::from(msg_value), u128::from(self.max_ton_msg_value_nano))?;
				deny_over(network, FEE_BUDGET, "forward_ton_amount", u128::from(forward_ton_amount), u128::from(self.max_ton_forward_nano))?;
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
/// `T` is the unsigned integer the ceiling is compared in (`u64` for a gas limit, `u128` for
/// a wei amount); either way `0` cannot be typed to mean "off".
fn env_cap<T>(lookup: &impl Fn(&str) -> Option<String>, name: &str, default: T) -> color_eyre::Result<T>
where
	T: std::str::FromStr + Default + PartialEq, {
	match lookup(name).filter(|raw| !raw.is_empty()) {
		Some(raw) => {
			let value = raw
				.trim()
				.parse::<T>()
				.map_err(|_| color_eyre::eyre::eyre!("{name} must be a positive whole number, got {raw:?}"))?;
			if value == T::default() {
				return Err(color_eyre::eyre::eyre!("{name} must be positive — this control cannot be disabled"));
			}
			Ok(value)
		}
		None => Ok(default),
	}
}

/// The control names `deny_over` reports, so a refusal says which ceiling it hit.
const FEE_BUDGET: &str = "fee budget";
const GAS_TOPUP: &str = "gas top-up";

fn deny_over(network: Network, control: &str, field: &str, value: u128, cap: u128) -> Result<(), Status> {
	if value > cap {
		return Err(Status::permission_denied(format!("{field} {value} exceeds the signer's {control} cap of {cap} on {network}")));
	}
	Ok(())
}

/// A native transfer out of a deposit wallet: no core flow does this (a sweep moves USDT, and
/// its gas arrives FROM the gas station), so there is nothing to allow.
pub fn refuse_deposit_native(network: Network) -> Status {
	Status::permission_denied(format!("a deposit wallet signs no native transfers on {network} — only USDT sweeps into the treasury"))
}

/// A token transfer out of the gas station: it holds only the native coin and only ever tops
/// up deposit wallets, so a token transfer from it is forged by construction.
pub fn refuse_gas_station_token(network: Network) -> Status {
	Status::permission_denied(format!("the gas station signs native top-ups only on {network} — never a token transfer"))
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
		let gas_topup = GasTopupCaps::from_lookup(lookup)?;
		Ok(Self {
			max_transfer_usdt,
			destination_allowlist,
			ton_treasury_jetton_wallet,
			fee_budget,
			gas_topup,
		})
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

	pub fn gas_topup(&self) -> &GasTopupCaps {
		&self.gas_topup
	}

	/// Enforce the fee budget on a transaction about to be signed — from ANY wallet.
	pub fn check_fee_budget(&self, network: Network, quote: FeeQuote) -> Result<(), Status> {
		self.fee_budget.check(network, quote)
	}

	// === treasury ================================================================

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
		self.check_allowlist(network, to_address)
	}

	/// Enforce the policy on a treasury-sourced NATIVE (gas-coin) transfer: only the
	/// destination allowlist applies — the per-transfer cap is USDT-denominated and cannot
	/// price a native amount. No core flow sends native funds FROM the treasury today (gas
	/// top-ups are signed from the gas-station wallet), so an operator enabling the
	/// allowlist must include any deliberate treasury-native destination on it.
	pub fn check_treasury_native_transfer(&self, network: Network, to_address: &str) -> Result<(), Status> {
		self.check_allowlist(network, to_address)
	}

	/// Enforce the policy on a treasury jetton transfer's `response_destination` — the address
	/// the excess Toncoin returns to, and so a second destination on the same signed message.
	/// Unlike `to_address` this is never a user's address: a legitimate withdrawal always points
	/// it at the treasury itself, so the check is ALWAYS on — the sending wallet's own address
	/// is accepted in either rendering (the raw `0:<hex>` the signer stores, the base64 the hub
	/// may carry), and anything else only if an allowlist is set and lists it.
	pub fn check_treasury_response_destination(&self, own_address: &str, response_destination: &str) -> Result<(), Status> {
		if provision::addresses_agree(Network::Ton, own_address, response_destination) || self.allowlisted(Network::Ton, response_destination) {
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
				.check_allowlist(Network::Ton, our_jetton_wallet)
				.map_err(|_| Status::permission_denied("our_jetton_wallet is neither pinned nor on the signer's allowlist")),
		}
	}

	fn check_allowlist(&self, network: Network, to_address: &str) -> Result<(), Status> {
		if !self.destination_allowlist.is_empty() && !self.allowlisted(network, to_address) {
			return Err(Status::permission_denied("treasury transfer destination is not on the signer's allowlist"));
		}
		Ok(())
	}

	/// Rendering-aware membership: an operator may list an address in any spelling the
	/// network accepts (EIP-55 or lowercase, TON raw or base64) and the hub may send another.
	fn allowlisted(&self, network: Network, address: &str) -> bool {
		self.destination_allowlist.iter().any(|listed| provision::addresses_agree(network, listed, address))
	}

	// === deposit (sweep) ==========================================================

	/// Enforce the sweep rule on a token transfer signed from a deposit wallet: the only
	/// legitimate destination is the treasury's own address on `network`, which the caller
	/// reads from `wallet_secrets` (`None` ⇒ the treasury is not provisioned there, so there is
	/// nowhere to sweep to and the request is refused).
	pub fn check_sweep_destination(&self, network: Network, treasury_address: Option<&str>, to_address: &str) -> Result<(), Status> {
		match treasury_address {
			Some(treasury) if provision::addresses_agree(network, treasury, to_address) => Ok(()),
			Some(_) => Err(Status::permission_denied(format!(
				"a deposit wallet may only sweep to the treasury on {network}; to_address is not the treasury"
			))),
			None => Err(Status::permission_denied(format!("the treasury is not provisioned on {network} — nothing to sweep to"))),
		}
	}

	/// Enforce the sweep rule on a jetton sweep's `response_destination`: the excess Toncoin
	/// may return to the sending wallet itself, to the gas station (where the hub sends it, so
	/// the station is topped back up) or to the treasury — all three derived by the signer,
	/// the latter two `None` when not provisioned on TON.
	pub fn check_sweep_response_destination(&self, own_address: &str, gas_station: Option<&str>, treasury: Option<&str>, response_destination: &str) -> Result<(), Status> {
		let ours = std::iter::once(own_address).chain(gas_station).chain(treasury);
		if ours.into_iter().any(|address| provision::addresses_agree(Network::Ton, address, response_destination)) {
			return Ok(());
		}
		Err(Status::permission_denied(
			"sweep response_destination must be the sending wallet, the gas station or the treasury",
		))
	}

	// === gas station (top-up) =====================================================

	/// Enforce the top-up rule on a native transfer signed from the gas station: the
	/// destination must be an address the signer itself holds a key for on `network`
	/// (`held_address` is the active `wallet_secrets` row the caller found for `to_address`,
	/// `None` when there is none), and the drip may not exceed the rail's [`GasTopupCaps`]
	/// ceiling.
	pub fn check_gas_topup(&self, network: Network, to_address: &str, held_address: Option<&str>, amount: u128) -> Result<(), Status> {
		match held_address {
			Some(held) if provision::addresses_agree(network, held, to_address) => {}
			_ => {
				return Err(Status::permission_denied(format!(
					"gas top-up destination is not an address this signer holds a key for on {network}"
				)));
			}
		}
		deny_over(network, GAS_TOPUP, "amount", amount, self.gas_topup.cap(network))
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
			gas_topup: GasTopupCaps::default(),
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
		assert!(p.check_treasury_native_transfer(Network::Bep20, "0xgood").is_ok());
		assert!(p.check_treasury_native_transfer(Network::Bep20, "0xbad").is_err());
		// Allowlist unset → no-op, even with a cap configured.
		let p = policy(Some(1), &[]);
		assert!(p.check_treasury_native_transfer(Network::Bep20, "0xanything").is_ok());
	}

	// === allowlist: rendering-aware membership ===================================

	const EIP55: &str = "0x7E5F4552091A69125d5DfCb7b8C2659029395Bdf";
	const OTHER_EVM: &str = "0x024da544a76714a3812096e9ef84d40b2c8863e8";
	const TRON: &str = "TJRabPrwbZy45sbavfcjinPJC18kjpRTv8";

	#[test]
	fn allowlist_membership_ignores_eip55_casing_on_evm() {
		let p = policy(None, &[EIP55]);
		assert!(p.check_treasury_transfer(Network::Bep20, &EIP55.to_ascii_lowercase(), 1).is_ok());
		assert!(p.check_treasury_transfer(Network::Polygon, &EIP55.to_ascii_uppercase().replace("0X", "0x"), 1).is_ok());
		// The list may be spelled lowercase while the hub sends EIP-55.
		assert!(policy(None, &[&EIP55.to_ascii_lowercase()]).check_treasury_native_transfer(Network::Bep20, EIP55).is_ok());
		denied(p.check_treasury_transfer(Network::Bep20, OTHER_EVM, 1));
	}

	#[test]
	fn allowlist_membership_spans_ton_raw_and_base64_but_keeps_tron_case_sensitive() {
		let raw = own_raw();
		assert!(policy(None, &[&raw]).check_treasury_transfer(Network::Ton, OWN_BASE64, 1).is_ok());
		assert!(policy(None, &[OWN_BASE64]).check_treasury_transfer(Network::Ton, &raw, 1).is_ok());
		denied(policy(None, &[OWN_BASE64]).check_treasury_transfer(Network::Ton, FOREIGN, 1));
		// Tron's Base58Check is case-sensitive: a re-cased string is a different (invalid) address.
		assert!(policy(None, &[TRON]).check_treasury_transfer(Network::Trc20, TRON, 1).is_ok());
		denied(policy(None, &[TRON]).check_treasury_transfer(Network::Trc20, &TRON.to_ascii_lowercase(), 1));
	}

	// === deposit: sweep ===========================================================

	#[test]
	fn sweep_destination_must_be_the_treasury_in_any_rendering() {
		let p = SignerPolicy::default();
		assert!(p.check_sweep_destination(Network::Bep20, Some(EIP55), &EIP55.to_ascii_lowercase()).is_ok());
		assert!(p.check_sweep_destination(Network::Ton, Some(&own_raw()), OWN_BASE64).is_ok());
		let status = denied(p.check_sweep_destination(Network::Bep20, Some(EIP55), OTHER_EVM));
		assert!(status.message().contains("treasury"), "{status:?}");
		// No treasury on the network: nowhere to sweep to, so nothing is signed.
		let status = denied(p.check_sweep_destination(Network::Ton, None, OWN_BASE64));
		assert!(status.message().contains("not provisioned"), "{status:?}");
	}

	#[test]
	fn sweep_response_destination_admits_self_station_and_treasury_only() {
		let p = SignerPolicy::default();
		let own = own_raw();
		let station = JETTON_WALLET_RAW;
		let treasury = FOREIGN;
		assert!(p.check_sweep_response_destination(&own, Some(station), Some(treasury), OWN_BASE64).is_ok());
		assert!(p.check_sweep_response_destination(&own, Some(station), Some(treasury), &jetton_wallet_base64()).is_ok());
		assert!(p.check_sweep_response_destination(&own, Some(station), Some(treasury), treasury).is_ok());
		// Neither reserved wallet provisioned on TON: only the sender itself remains.
		assert!(p.check_sweep_response_destination(&own, None, None, &own).is_ok());
		denied(p.check_sweep_response_destination(&own, None, None, station));
		denied(p.check_sweep_response_destination(&own, Some(station), Some(treasury), "not-an-address"));
	}

	#[test]
	fn deposit_native_and_gas_station_token_are_refused_outright() {
		assert_eq!(refuse_deposit_native(Network::Bep20).code(), Code::PermissionDenied);
		assert_eq!(refuse_gas_station_token(Network::Ton).code(), Code::PermissionDenied);
	}

	// === gas station: top-up ======================================================

	#[test]
	fn gas_topup_requires_a_held_destination_and_a_bounded_drip() {
		let p = SignerPolicy::default();
		// Held (found in wallet_secrets, spelled differently) and under the cap.
		assert!(p.check_gas_topup(Network::Bep20, &EIP55.to_ascii_lowercase(), Some(EIP55), 50_000_000_000_000_000).is_ok());
		let over = denied(p.check_gas_topup(Network::Bep20, EIP55, Some(EIP55), 50_000_000_000_000_001));
		assert!(over.message().contains("gas top-up"), "{over:?}");
		// Per-rail caps: 2 POL, 50 TRX, 0.2 TON.
		assert!(p.check_gas_topup(Network::Polygon, EIP55, Some(EIP55), 2_000_000_000_000_000_000).is_ok());
		denied(p.check_gas_topup(Network::Polygon, EIP55, Some(EIP55), 2_000_000_000_000_000_001));
		assert!(p.check_gas_topup(Network::Trc20, TRON, Some(TRON), 50_000_000).is_ok());
		denied(p.check_gas_topup(Network::Trc20, TRON, Some(TRON), 50_000_001));
		assert!(p.check_gas_topup(Network::Ton, OWN_BASE64, Some(&own_raw()), 200_000_000).is_ok());
		denied(p.check_gas_topup(Network::Ton, OWN_BASE64, Some(&own_raw()), 200_000_001));
		// Not held: refused whatever the amount — and a lookup that returned a DIFFERENT
		// address (a caller bug) is not "held" either.
		let status = denied(p.check_gas_topup(Network::Bep20, EIP55, None, 1));
		assert!(status.message().contains("holds a key"), "{status:?}");
		denied(p.check_gas_topup(Network::Bep20, EIP55, Some(OTHER_EVM), 1));
	}

	#[test]
	fn gas_topup_caps_come_from_env_and_refuse_zero() {
		assert_eq!(GasTopupCaps::from_lookup(&lookup(&[])).unwrap(), GasTopupCaps::default());
		let caps = GasTopupCaps::from_lookup(&lookup(&[
			("SIGNER_MAX_GAS_TOPUP_BEP20", "1"),
			("SIGNER_MAX_GAS_TOPUP_POLYGON", "340282366920938463463374607431768211455"),
			("SIGNER_MAX_GAS_TOPUP_TRC20", "3"),
			("SIGNER_MAX_GAS_TOPUP_TON", "4"),
		]))
		.unwrap();
		assert_eq!(
			caps,
			GasTopupCaps {
				bep20_wei: 1,
				polygon_wei: u128::MAX,
				trc20_sun: 3,
				ton_nano: 4,
			}
		);
		for name in [
			"SIGNER_MAX_GAS_TOPUP_BEP20",
			"SIGNER_MAX_GAS_TOPUP_POLYGON",
			"SIGNER_MAX_GAS_TOPUP_TRC20",
			"SIGNER_MAX_GAS_TOPUP_TON",
		] {
			for bad in ["0", "abc", "-5", "1.5"] {
				let err = GasTopupCaps::from_lookup(&lookup(&[(name, bad)])).expect_err(&format!("{name}={bad} must not boot"));
				assert!(err.to_string().contains(name), "{err}");
			}
		}
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
		assert!(p.treasury_jetton_wallet_pinned());
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
		assert_eq!(p.max_transfer_usdt(), None);
		assert_eq!(p.allowlist_len(), 0);
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
