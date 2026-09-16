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
//!     three derived by the signer. `our_jetton_wallet` cannot be derived (it is the user's
//!     jetton wallet, a contract address the hub resolves through an indexer and the signer
//!     never computes); it is **pinned on first use** instead — see the jetton wallet rule
//!     below.
//!   - **Gas station** (`Uuid::from_u128(1)`, the hub's reserved id): it only ever tops up
//!     deposit wallets with the native coin, so it signs **native transfers only**, the
//!     destination must be an address the signer itself holds a key for on that network
//!     (again derived from `wallet_secrets`, not configured), and the drip is capped by
//!     [`GasTopupCaps`].
//!   - **Treasury** (`Uuid::nil()`): the one class with an arbitrary destination — user
//!     withdrawals — and so the one where the amount controls live:
//!       - a **pinned token** ([`TokenPins`], `SIGNER_USDT_CONTRACT_{BEP20,POLYGON,TRC20}`,
//!         defaulting to mainnet USDT): a treasury token transfer's `token_contract` must be
//!         the pinned contract, compared rendering-aware. The USDT cap below is scaled by
//!         USDT's decimals, so applied to any other token it would bound nothing (an
//!         8-decimal token would get a cap of `N × 10^10` whole units); pinning the contract is
//!         what makes the cap mean what its name says. On TON the same role is played by the
//!         pinned jetton wallet. Sweeps are not pinned — their destination is the treasury;
//!       - a **per-transfer USDT cap** (`SIGNER_MAX_TRANSFER_USDT`, default 100) — a single
//!         signed treasury transfer can move at most this much, so one forged request can't
//!         drain the hot wallet. Always on: unset is the default, `0` is a boot error;
//!       - a **USDT window** (`SIGNER_MAX_TREASURY_USDT_PER_HOUR`, default 1_000) — the most
//!         USDT the treasury may pay out on one rail over a sliding hour, counted in the
//!         signer's own ledger ([`crate::native_spend`], `asset = 'usdt'`) the way the native
//!         window below is. The per-transfer cap bounds one payout; this bounds how many of
//!         them a compromised hub gets before an operator notices;
//!       - an optional **destination allowlist** (`SIGNER_DESTINATION_ALLOWLIST`) — when set,
//!         treasury transfers may only go to pre-registered addresses (a hardened/staged
//!         posture; off by default, since the normal withdrawal model sends to arbitrary user
//!         addresses). Membership is rendering-aware ([`provision::addresses_agree`]): EIP-55
//!         casing and TON's raw-vs-base64 forms are the same address, so an operator may list
//!         either spelling. A TON jetton transfer names two more addresses that receive native
//!         Toncoin, and both are held to the same list: the `response_destination` (the excess
//!         returns there — always the sending wallet itself on a legitimate withdrawal, so that
//!         is accepted with or without a list) and `our_jetton_wallet` (the internal message's
//!         destination, which receives `msg_value` — pinned, never allowlisted; below).
//!
//! **The jetton wallet is pinned, for every class** ([`check_jetton_wallet_pin`]). A jetton
//! transfer is an internal message TO `our_jetton_wallet` carrying `msg_value` in Toncoin,
//! and whatever contract sits at that address receives it. The treasury's is pinned by the
//! operator (`SIGNER_TON_TREASURY_JETTON_WALLET`) when set; otherwise — and for every deposit
//! wallet always — the first transfer a `(wallet, network)` signs pins the address it named
//! in the signer's own `jetton_wallets` table ([`crate::jetton_wallets`]), and every later
//! transfer must agree with it (rendering-aware). The allowlist plays no part: a jetton
//! wallet is the wallet's identity, not a destination. **Residual risk:** the first message
//! per wallet — each deposit wallet's first sweep, and the treasury's first withdrawal after
//! the deploy if the operator pin is unset — trusts the hub once, for at most the fee
//! budget's `msg_value` ceiling (0.1 TON). Deriving the address from the jetton master's
//! StateInit is the follow-up that removes even that.
//!
//! **Native (gas-coin) transfers from the treasury are refused by default.** No core flow
//! sends native funds FROM the treasury (gas top-ups are signed from the gas station), and
//! the USDT cap cannot price a native amount, so before this the treasury's whole native
//! balance — the gas float on all four rails — was one forged request away. An operator who
//! needs the flow opts in with `SIGNER_ALLOW_TREASURY_NATIVE=true`, which then REQUIRES a
//! non-empty **treasury native allowlist** (`SIGNER_TREASURY_NATIVE_ALLOWLIST`) and a
//! ceiling in the network's base units (`SIGNER_MAX_TREASURY_NATIVE_{BEP20,POLYGON,TRC20,TON}`;
//! a network without one stays refused) — the boot fails otherwise, since "allowed,
//! unbounded, to anywhere" is the hole this closes. The native allowlist is its own list on
//! purpose: the USDT allowlist above gates every user withdrawal, so requiring IT for a
//! native opt-in would make an operator who only wants to refill the gas station park every
//! payout. The two lists never read each other. Both are **no-ops until configured** — the
//! normal withdrawal model sends to arbitrary user addresses.
//!
//! Everything else is **on by default** with ceilings an honest hub never reaches, and an
//! operator may raise them via their `SIGNER_MAX_*` variables but cannot switch them off
//! (`0` is a boot error, not "disabled"). Besides the two USDT ceilings above:
//!
//!   - the **fee budget** ([`FeeBudget`]) — a ceiling on the gas/fee side of EVERY signed
//!     transaction, on every wallet class. The amount caps above bound what leaves in USDT;
//!     they say nothing about the native coin a transaction burns as fee. Without this gate a
//!     forged request moving 1 USDT with an absurd `gas_price` would hand the whole native
//!     balance to the miner;
//!   - the **gas top-up cap** ([`GasTopupCaps`]) — a ceiling on the native amount one
//!     gas-station drip may carry;
//!   - the **native spend window** ([`NativeSpendWindow`]) — a ceiling on the native coin a
//!     `(wallet, network)` may commit to over a sliding hour, counted in the signer's own
//!     database ([`crate::native_spend`]) before each signature. The two ceilings above are
//!     per signature; nothing else bounds how many signatures a compromised hub asks for, and
//!     a thousand drips or fee-limits at the ceiling are the same hole as one unbounded
//!     signature. What counts is the native coin a signature can burn or move: EVM
//!     `gas_price × gas_limit` plus a native transfer's `value`, Tron `fee_limit` or the TRX
//!     amount, TON `msg_value` or the Toncoin amount.
//!
//! **A window refusal parks the withdrawal.** The two windows (this one and the treasury
//! USDT window) refuse with `permission_denied` like every other rule here, and the hub
//! treats that as a policy verdict: a withdrawal is parked (`custody.rs` in the hub), a
//! sweep backs off. Deliberate — a retryable code would have the hub hammer the same window
//! and block its whole outbox for up to an hour behind one request. Treat a window refusal
//! as an alert: check the volume that filled it, raise the ceiling deliberately or unpark.
//!
//! **Tron signing is off by default** (`SIGNER_TRON_SIGNING_ENABLED`): the hub keeps the rail
//! frozen (`TRC20_FROZEN` in `piggybank/core/src/config.rs`), so no legitimate Tron signature
//! exists today and every Tron handler refuses before any other check. Flip it together with
//! the hub's freeze.
//!
//! `Status` is tonic's large error type we don't control (same as the service handlers).
#![allow(clippy::result_large_err)]

use std::{str::FromStr as _, time::Duration};

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

/// The default per-transfer treasury cap, in whole USDT: what production runs
/// (`SIGNER_MAX_TRANSFER_USDT` in `flake.nix`), so an unset variable is that posture rather
/// than "unbounded". Sized to the hot float, not to ambition — raise it as liquidity grows.
const DEFAULT_MAX_TRANSFER_USDT: u64 = 100;

/// The default treasury USDT window, in whole USDT per rail per sliding hour: ten payouts at
/// the per-transfer cap. An honest hour of withdrawals is a handful; a compromised hub
/// asking for the ceiling back to back gets ten of them, not an unbounded stream, before an
/// operator's alert fires. Raised together with the float via
/// `SIGNER_MAX_TREASURY_USDT_PER_HOUR`.
const DEFAULT_MAX_TREASURY_USDT_PER_HOUR: u64 = 1_000;

/// The signer's spend policy, loaded once at boot and consulted on every signing request.
/// [`Default`] is the production posture with every variable unset — not "everything off".
#[derive(Clone, Debug)]
pub struct SignerPolicy {
	/// Max USDT (whole units) a single treasury transfer may move. Always on.
	max_transfer_usdt: u64,
	/// Max USDT (whole units) the treasury may pay out per rail over [`SPEND_WINDOW`].
	treasury_usdt_per_hour: u64,
	/// If non-empty, a treasury USDT transfer's destination must agree with one of these (wire
	/// address strings in any rendering — membership goes through
	/// [`provision::addresses_agree`], not string equality). Empty ⇒ any destination is
	/// allowed (the default withdrawal model). Never consulted for a native transfer.
	destination_allowlist: Vec<String>,
	/// The destinations a native transfer FROM the treasury may go to, once opted in; same
	/// membership rule, its own list (`SIGNER_TREASURY_NATIVE_ALLOWLIST`). Never consulted
	/// for a USDT transfer.
	treasury_native_allowlist: Vec<String>,
	/// The treasury's own USDT jetton wallet, when the operator pinned it: a treasury jetton
	/// transfer's `our_jetton_wallet` must be this address. `None` ⇒ pinned on first use like
	/// every other wallet's.
	ton_treasury_jetton_wallet: Option<String>,
	/// The always-on ceiling on the fee side of every signed transaction.
	fee_budget: FeeBudget,
	/// The always-on ceiling on the native amount one gas-station drip may carry.
	gas_topup: GasTopupCaps,
	/// The token contract a treasury token transfer must name, per rail.
	token_pins: TokenPins,
	/// `Some` only when an operator opted in to native transfers FROM the treasury; carries
	/// the per-rail ceilings. `None` ⇒ every such transfer is refused.
	treasury_native: Option<TreasuryNativeCaps>,
	/// The always-on ceiling on native spend per `(wallet, network)` over [`SPEND_WINDOW`].
	native_spend: NativeSpendWindow,
	/// Whether the Tron handlers sign at all (`SIGNER_TRON_SIGNING_ENABLED`, default off).
	tron_signing_enabled: bool,
}

impl Default for SignerPolicy {
	fn default() -> Self {
		Self {
			max_transfer_usdt: DEFAULT_MAX_TRANSFER_USDT,
			treasury_usdt_per_hour: DEFAULT_MAX_TREASURY_USDT_PER_HOUR,
			destination_allowlist: Vec::new(),
			treasury_native_allowlist: Vec::new(),
			ton_treasury_jetton_wallet: None,
			fee_budget: FeeBudget::default(),
			gas_topup: GasTopupCaps::default(),
			token_pins: TokenPins::default(),
			treasury_native: None,
			native_spend: NativeSpendWindow::default(),
			tron_signing_enabled: false,
		}
	}
}

/// The sliding window every ledger ceiling — [`NativeSpendWindow`] and the treasury USDT
/// window — is measured over.
pub const SPEND_WINDOW: Duration = Duration::from_secs(60 * 60);

/// Ceilings on the native coin one `(wallet, network)` may commit to per sliding hour, in
/// base units (`SIGNER_MAX_NATIVE_SPEND_PER_HOUR_{BEP20,POLYGON,TRC20,TON}`).
///
/// Sized as the largest single-signature native spend the other two ceilings admit, times a
/// generous count of honest signatures per hour — the busiest wallet is the gas station,
/// which may drip to tens of fresh deposit addresses in one sweep cycle:
///   - BEP20: a drip is at most 0.05 BNB ([`GasTopupCaps`]) + 0.01 BNB fee ([`FeeBudget`])
///     = 0.06 BNB; a sweep or payout at most 0.01 BNB. Cap 1e18 wei (1 BNB): ~16 ceiling
///     drips, or ~100 ceiling sweeps — while an honest hour at normal gas is ~0.1 BNB.
///   - Polygon: a drip at most 2 POL + 0.5 POL; a sweep at most 0.5 POL. Cap 5e19 wei
///     (50 POL): 20 ceiling drips or 100 ceiling sweeps.
///   - Tron: a TRC20 signature burns up to `fee_limit` 100 TRX; a drip 50 TRX. Cap 2e9 SUN
///     (2_000 TRX): 20 ceiling sweeps or 40 drips.
///   - TON: a jetton send attaches 0.1 TON; a drip 0.2 TON. Cap 5e9 nanoton (5 TON): 25
///     ceiling drips or 50 sweeps.
///
/// Each is per wallet, so the treasury, the gas station and every deposit wallet get their
/// own hour; an operator sizing the gas float tighter lowers the matching variable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeSpendWindow {
	bep20_wei: u128,
	polygon_wei: u128,
	trc20_sun: u128,
	ton_nano: u128,
}

impl Default for NativeSpendWindow {
	fn default() -> Self {
		Self {
			bep20_wei: 1_000_000_000_000_000_000,
			polygon_wei: 50_000_000_000_000_000_000,
			trc20_sun: 2_000_000_000,
			ton_nano: 5_000_000_000,
		}
	}
}

impl NativeSpendWindow {
	fn from_lookup(lookup: &impl Fn(&str) -> Option<String>) -> color_eyre::Result<Self> {
		let defaults = Self::default();
		Ok(Self {
			bep20_wei: env_cap(lookup, "SIGNER_MAX_NATIVE_SPEND_PER_HOUR_BEP20", defaults.bep20_wei)?,
			polygon_wei: env_cap(lookup, "SIGNER_MAX_NATIVE_SPEND_PER_HOUR_POLYGON", defaults.polygon_wei)?,
			trc20_sun: env_cap(lookup, "SIGNER_MAX_NATIVE_SPEND_PER_HOUR_TRC20", defaults.trc20_sun)?,
			ton_nano: env_cap(lookup, "SIGNER_MAX_NATIVE_SPEND_PER_HOUR_TON", defaults.ton_nano)?,
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

/// The USDT contract per rail (`SIGNER_USDT_CONTRACT_{BEP20,POLYGON,TRC20}`), defaulting to
/// mainnet USDT — the same defaults as the hub's `BSC_USDT_CONTRACT`/`POLYGON_USDT_CONTRACT`/
/// `TRON_USDT_CONTRACT` (`piggybank/core/src/config.rs`), pinned HERE so a compromised hub
/// cannot move a different token under the USDT cap. An override that does not parse as an
/// address of its network fails the boot. TON has no entry: the jetton wallet pin covers it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TokenPins {
	bep20: String,
	polygon: String,
	trc20: String,
}

impl Default for TokenPins {
	fn default() -> Self {
		Self {
			bep20: "0x55d398326f99059fF775485246999027B3197955".to_owned(),
			polygon: "0xc2132D05D31c914a87C6611C10748AEb04B58e8F".to_owned(),
			trc20: "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t".to_owned(),
		}
	}
}

impl TokenPins {
	fn from_lookup(lookup: &impl Fn(&str) -> Option<String>) -> color_eyre::Result<Self> {
		let defaults = Self::default();
		Ok(Self {
			bep20: env_address(lookup, "SIGNER_USDT_CONTRACT_BEP20", Network::Bep20, defaults.bep20)?,
			polygon: env_address(lookup, "SIGNER_USDT_CONTRACT_POLYGON", Network::Polygon, defaults.polygon)?,
			trc20: env_address(lookup, "SIGNER_USDT_CONTRACT_TRC20", Network::Trc20, defaults.trc20)?,
		})
	}

	fn pinned(&self, network: Network) -> Option<&str> {
		match network {
			Network::Bep20 => Some(&self.bep20),
			Network::Polygon => Some(&self.polygon),
			Network::Trc20 => Some(&self.trc20),
			Network::Ton => None,
		}
	}
}

/// Ceilings on a native transfer FROM the treasury, per rail, in base units — present only
/// once an operator opted in (`SIGNER_ALLOW_TREASURY_NATIVE=true`). There is no default: no
/// honest flow exists to size one from, so a rail the operator did not cap
/// (`SIGNER_MAX_TREASURY_NATIVE_<RAIL>` unset) stays refused.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TreasuryNativeCaps {
	bep20_wei: Option<u128>,
	polygon_wei: Option<u128>,
	trc20_sun: Option<u128>,
	ton_nano: Option<u128>,
}

impl TreasuryNativeCaps {
	fn from_lookup(lookup: &impl Fn(&str) -> Option<String>) -> color_eyre::Result<Self> {
		Ok(Self {
			bep20_wei: env_cap_opt(lookup, "SIGNER_MAX_TREASURY_NATIVE_BEP20")?,
			polygon_wei: env_cap_opt(lookup, "SIGNER_MAX_TREASURY_NATIVE_POLYGON")?,
			trc20_sun: env_cap_opt(lookup, "SIGNER_MAX_TREASURY_NATIVE_TRC20")?,
			ton_nano: env_cap_opt(lookup, "SIGNER_MAX_TREASURY_NATIVE_TON")?,
		})
	}

	fn cap(&self, network: Network) -> Option<u128> {
		match network {
			Network::Bep20 => self.bep20_wei,
			Network::Polygon => self.polygon_wei,
			Network::Trc20 => self.trc20_sun,
			Network::Ton => self.ton_nano,
		}
	}

	fn is_empty(&self) -> bool {
		*self == Self::default()
	}
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

/// A whole-USDT ceiling lowered to `network`'s on-chain base units, so it compares
/// like-for-like with a wire amount. A whole number of USDT is representable on every rail,
/// so the dust refusal is unreachable; it maps to `internal` because reaching it would be
/// our bug, not a policy verdict.
fn usdt_cap_onchain(network: Network, whole_usdt: u64) -> Result<u128, Status> {
	Usdt::from_base_units(u128::from(whole_usdt).saturating_mul(CANONICAL_PER_USDT))
		.to_onchain(network)
		.map_err(|_| Status::internal("signer cap is not representable on this network"))
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
	env_cap_opt(lookup, name).map(|value| value.unwrap_or(default))
}

/// An OPTIONAL ceiling from `lookup`: unset/empty ⇒ `None`; set ⇒ a positive whole number,
/// where `0`, negative or unparsable is an error — a typo must fail the boot, not silently
/// read as "unset".
fn env_cap_opt<T>(lookup: &impl Fn(&str) -> Option<String>, name: &str) -> color_eyre::Result<Option<T>>
where
	T: std::str::FromStr + Default + PartialEq, {
	let Some(raw) = lookup(name).filter(|raw| !raw.is_empty()) else {
		return Ok(None);
	};
	let value = raw
		.trim()
		.parse::<T>()
		.map_err(|_| color_eyre::eyre::eyre!("{name} must be a positive whole number, got {raw:?}"))?;
	if value == T::default() {
		return Err(color_eyre::eyre::eyre!("{name} must be positive — this control cannot be disabled"));
	}
	Ok(Some(value))
}

/// A comma-separated list from `lookup`, trimmed, blanks dropped; unset ⇒ empty.
fn env_list(lookup: &impl Fn(&str) -> Option<String>, name: &str) -> Vec<String> {
	lookup(name)
		.map(|raw| raw.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect())
		.unwrap_or_default()
}

/// Rendering-aware membership in an allowlist: an operator may list an address in any
/// spelling the network accepts (EIP-55 or lowercase, TON raw or base64) and the hub may
/// send another.
fn listed(list: &[String], network: Network, address: &str) -> bool {
	list.iter().any(|listed| provision::addresses_agree(network, listed, address))
}

/// A boolean opt-in from `lookup`: unset/empty ⇒ `false`; `true`/`false` (any case) ⇒ that;
/// anything else ⇒ error, so a mistyped `ture` cannot pass for either.
fn env_flag(lookup: &impl Fn(&str) -> Option<String>, name: &str) -> color_eyre::Result<bool> {
	match lookup(name).map(|raw| raw.trim().to_ascii_lowercase()).filter(|raw| !raw.is_empty()) {
		Some(raw) if raw == "true" => Ok(true),
		Some(raw) if raw == "false" => Ok(false),
		Some(raw) => Err(color_eyre::eyre::eyre!("{name} must be `true` or `false`, got {raw:?}")),
		None => Ok(false),
	}
}

/// An address of `network` from `lookup`: unset/empty ⇒ `default`; set but not an address of
/// that network ⇒ error (a pin that cannot match would refuse every treasury transfer).
fn env_address(lookup: &impl Fn(&str) -> Option<String>, name: &str, network: Network, default: String) -> color_eyre::Result<String> {
	match lookup(name).map(|raw| raw.trim().to_owned()).filter(|raw| !raw.is_empty()) {
		Some(raw) if is_address(network, &raw) => Ok(raw),
		Some(raw) => Err(color_eyre::eyre::eyre!("{name} is not a {network} address: {raw:?}")),
		None => Ok(default),
	}
}

/// Does `value` parse as an address of `network`? The same parsers the handlers use on the
/// wire, so a pin that boots is a pin the handlers can compare against.
fn is_address(network: Network, value: &str) -> bool {
	match network {
		Network::Bep20 | Network::Polygon => crate::evm_tx::parse_address(value).is_some(),
		Network::Trc20 => crate::key_vault::tron_base58_to_raw(value).is_some(),
		Network::Ton => tonlib_core::TonAddress::from_str(value).is_ok(),
	}
}

/// The control names `deny_over` reports, so a refusal says which ceiling it hit.
const FEE_BUDGET: &str = "fee budget";
const GAS_TOPUP: &str = "gas top-up";
const TREASURY_NATIVE: &str = "treasury native";

fn deny_over(network: Network, control: &str, field: &str, value: u128, cap: u128) -> Result<(), Status> {
	if value > cap {
		return Err(Status::permission_denied(format!("{field} {value} exceeds the signer's {control} cap of {cap} on {network}")));
	}
	Ok(())
}

/// The native coin an EVM signature commits to: the whole fee (`gas_price × gas_limit` — the
/// chain refunds unused gas, but the signature authorises all of it) plus the native `value`.
/// The fee budget has already refused an overflowing product; the sum is checked here.
pub fn evm_native_spend(network: Network, gas_price: u128, gas_limit: u64, value: u128) -> Result<u128, Status> {
	gas_price
		.checked_mul(u128::from(gas_limit))
		.and_then(|fee| fee.checked_add(value))
		.ok_or_else(|| Status::permission_denied(format!("native spend of the transaction overflows the signer's window on {network}")))
}

/// Who pinned a jetton wallet — only the refusal's wording differs, so an operator reading
/// the hub's parked withdrawal knows which pin to look at.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JettonPinSource {
	/// `SIGNER_TON_TREASURY_JETTON_WALLET`.
	Operator,
	/// The `jetton_wallets` row the wallet's first transfer wrote.
	FirstUse,
}

/// Enforce a jetton transfer's `our_jetton_wallet` against the pin held for the sending
/// wallet — the internal message's destination, which receives `msg_value` in native Toncoin
/// whatever contract sits there. Pure: the caller resolved which pin applies (the operator's
/// for the treasury when set, else the first-use row) and passes it in; the comparison is
/// rendering-aware, so the raw and base64 spellings of one address agree.
pub fn check_jetton_wallet_pin(pinned: &str, our_jetton_wallet: &str, source: JettonPinSource) -> Result<(), Status> {
	if provision::addresses_agree(Network::Ton, pinned, our_jetton_wallet) {
		return Ok(());
	}
	Err(Status::permission_denied(match source {
		JettonPinSource::Operator => "our_jetton_wallet is not the treasury's jetton wallet pinned by SIGNER_TON_TREASURY_JETTON_WALLET",
		JettonPinSource::FirstUse => "our_jetton_wallet is not the jetton wallet this wallet pinned on first use",
	}))
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
		let max_transfer_usdt = env_cap(lookup, "SIGNER_MAX_TRANSFER_USDT", DEFAULT_MAX_TRANSFER_USDT)?;
		let treasury_usdt_per_hour = env_cap(lookup, "SIGNER_MAX_TREASURY_USDT_PER_HOUR", DEFAULT_MAX_TREASURY_USDT_PER_HOUR)?;
		let destination_allowlist = env_list(lookup, "SIGNER_DESTINATION_ALLOWLIST");
		let treasury_native_allowlist = env_list(lookup, "SIGNER_TREASURY_NATIVE_ALLOWLIST");
		let ton_treasury_jetton_wallet = match lookup("SIGNER_TON_TREASURY_JETTON_WALLET").map(|raw| raw.trim().to_owned()).filter(|s| !s.is_empty()) {
			// A pin that does not parse would refuse every TON withdrawal; fail the boot instead.
			Some(raw) if tonlib_core::TonAddress::from_str(&raw).is_ok() => Some(raw),
			Some(raw) => return Err(color_eyre::eyre::eyre!("SIGNER_TON_TREASURY_JETTON_WALLET is not a TON address: {raw:?}")),
			None => None,
		};
		let fee_budget = FeeBudget::from_lookup(lookup)?;
		let gas_topup = GasTopupCaps::from_lookup(lookup)?;
		let token_pins = TokenPins::from_lookup(lookup)?;
		let treasury_native = if env_flag(lookup, "SIGNER_ALLOW_TREASURY_NATIVE")? {
			// Opting in without the two bounds is the exact hole the default closes; refuse to
			// boot rather than run "allowed, unbounded, to anywhere".
			if treasury_native_allowlist.is_empty() {
				return Err(color_eyre::eyre::eyre!("SIGNER_ALLOW_TREASURY_NATIVE=true requires a non-empty SIGNER_TREASURY_NATIVE_ALLOWLIST"));
			}
			let caps = TreasuryNativeCaps::from_lookup(lookup)?;
			if caps.is_empty() {
				return Err(color_eyre::eyre::eyre!(
					"SIGNER_ALLOW_TREASURY_NATIVE=true requires at least one SIGNER_MAX_TREASURY_NATIVE_{{BEP20,POLYGON,TRC20,TON}} ceiling"
				));
			}
			Some(caps)
		} else {
			None
		};
		let native_spend = NativeSpendWindow::from_lookup(lookup)?;
		let tron_signing_enabled = env_flag(lookup, "SIGNER_TRON_SIGNING_ENABLED")?;
		Ok(Self {
			max_transfer_usdt,
			treasury_usdt_per_hour,
			destination_allowlist,
			treasury_native_allowlist,
			ton_treasury_jetton_wallet,
			fee_budget,
			gas_topup,
			token_pins,
			treasury_native,
			native_spend,
			tron_signing_enabled,
		})
	}

	pub fn treasury_jetton_wallet_pinned(&self) -> bool {
		self.ton_treasury_jetton_wallet.is_some()
	}

	/// The operator's pin for the treasury's jetton wallet, when set. It outranks the
	/// first-use pin: with it set, the `jetton_wallets` table is not consulted for the
	/// treasury at all.
	pub fn treasury_jetton_wallet(&self) -> Option<&str> {
		self.ton_treasury_jetton_wallet.as_deref()
	}

	pub fn max_transfer_usdt(&self) -> u64 {
		self.max_transfer_usdt
	}

	pub fn treasury_usdt_per_hour(&self) -> u64 {
		self.treasury_usdt_per_hour
	}

	pub fn allowlist_len(&self) -> usize {
		self.destination_allowlist.len()
	}

	pub fn treasury_native_allowlist_len(&self) -> usize {
		self.treasury_native_allowlist.len()
	}

	pub fn fee_budget(&self) -> &FeeBudget {
		&self.fee_budget
	}

	pub fn gas_topup(&self) -> &GasTopupCaps {
		&self.gas_topup
	}

	pub fn token_pins(&self) -> &TokenPins {
		&self.token_pins
	}

	/// The treasury-native ceilings, `Some` only when an operator opted in.
	pub fn treasury_native(&self) -> Option<&TreasuryNativeCaps> {
		self.treasury_native.as_ref()
	}

	pub fn native_spend(&self) -> &NativeSpendWindow {
		&self.native_spend
	}

	pub fn tron_signing_enabled(&self) -> bool {
		self.tron_signing_enabled
	}

	/// The first gate on every Tron handler: refused outright while the rail is frozen.
	pub fn check_tron_signing(&self) -> Result<(), Status> {
		if self.tron_signing_enabled {
			return Ok(());
		}
		Err(Status::permission_denied(
			"Tron signing is disabled on this signer (SIGNER_TRON_SIGNING_ENABLED) while the rail is frozen",
		))
	}

	/// Enforce the native spend window on `network`: `spent` is what the wallet's window
	/// already holds (read under the ledger's lock), `spend` what this signature would add.
	/// Pure, so the caller can hold the ledger transaction across it.
	pub fn check_native_spend_window(&self, network: Network, spent: u128, spend: u128) -> Result<(), Status> {
		let cap = self.native_spend.cap(network);
		let total = spent
			.checked_add(spend)
			.ok_or_else(|| Status::permission_denied(format!("native spend {spend} overflows the signer's window on {network}")))?;
		if total > cap {
			return Err(Status::permission_denied(format!(
				"native spend {spend} would bring this wallet's last hour on {network} to {total}, over the signer's native spend window cap of {cap}"
			)));
		}
		Ok(())
	}

	/// Enforce the treasury USDT window on `network`: `spent` is what the treasury's window on
	/// that rail already holds in on-chain base units (read under the ledger's lock), `spend`
	/// the payout being decided, in the same units. Pure, like the native check above.
	pub fn check_treasury_usdt_window(&self, network: Network, spent: u128, spend: u128) -> Result<(), Status> {
		let cap = usdt_cap_onchain(network, self.treasury_usdt_per_hour)?;
		let total = spent
			.checked_add(spend)
			.ok_or_else(|| Status::permission_denied(format!("USDT spend {spend} overflows the signer's window on {network}")))?;
		if total > cap {
			return Err(Status::permission_denied(format!(
				"treasury transfer of {spend} would bring the treasury's last hour on {network} to {total}, over the signer's treasury USDT window cap of {} USDT ({cap} on {network})",
				self.treasury_usdt_per_hour
			)));
		}
		Ok(())
	}

	/// Enforce the fee budget on a transaction about to be signed — from ANY wallet.
	pub fn check_fee_budget(&self, network: Network, quote: FeeQuote) -> Result<(), Status> {
		self.fee_budget.check(network, quote)
	}

	// === treasury ================================================================

	/// Enforce the policy on a treasury-sourced USDT transfer. `token_contract` is the wire's
	/// contract on the EVM and Tron rails (`None` on TON, where the jetton wallet pin plays
	/// that role) and must be the pinned one. `amount_base_units` is the transfer amount in
	/// `network`'s on-chain decimals (as it will be signed), so the cap is compared
	/// like-for-like after lowering the whole-USDT limit to the chain's precision. A breach is
	/// `permission_denied` — a policy refusal, not a malformed request.
	pub fn check_treasury_transfer(&self, network: Network, token_contract: Option<&str>, to_address: &str, amount_base_units: u128) -> Result<(), Status> {
		match (self.token_pins.pinned(network), token_contract) {
			(Some(pinned), Some(token)) if provision::addresses_agree(network, pinned, token) => {}
			(Some(_), Some(_)) => return Err(Status::permission_denied(format!("token_contract is not the signer's pinned USDT contract on {network}"))),
			(None, None) => {}
			// The handlers know which rails carry a contract; a mismatch here is our bug.
			(Some(_), None) | (None, Some(_)) => return Err(Status::internal(format!("token pin and wire contract disagree on presence for {network}"))),
		}
		let cap = usdt_cap_onchain(network, self.max_transfer_usdt)?;
		if amount_base_units > cap {
			return Err(Status::permission_denied(format!(
				"treasury transfer of {amount_base_units} exceeds the signer's per-transfer cap of {} USDT ({cap} on {network})",
				self.max_transfer_usdt
			)));
		}
		self.check_allowlist(network, to_address)
	}

	/// Enforce the policy on a treasury-sourced NATIVE (gas-coin) transfer: refused unless an
	/// operator opted in (`SIGNER_ALLOW_TREASURY_NATIVE`), and then only to a destination on
	/// the native allowlist (`SIGNER_TREASURY_NATIVE_ALLOWLIST` — not the USDT one), under the
	/// rail's `SIGNER_MAX_TREASURY_NATIVE_*` ceiling. No core flow sends
	/// native funds FROM the treasury (gas top-ups are signed from the gas-station wallet), and
	/// the USDT cap cannot price a native amount, so "off" is the only safe default.
	pub fn check_treasury_native_transfer(&self, network: Network, to_address: &str, amount: u128) -> Result<(), Status> {
		let Some(caps) = &self.treasury_native else {
			return Err(Status::permission_denied(format!(
				"native transfers from the treasury are disabled on {network} (SIGNER_ALLOW_TREASURY_NATIVE)"
			)));
		};
		let Some(cap) = caps.cap(network) else {
			return Err(Status::permission_denied(format!(
				"native transfers from the treasury have no ceiling on {network} (SIGNER_MAX_TREASURY_NATIVE_*)"
			)));
		};
		// Straight membership: the empty-list-means-anywhere reading is the withdrawal model's,
		// never this flow's (loading refuses the opt-in with no list).
		if !listed(&self.treasury_native_allowlist, network, to_address) {
			return Err(Status::permission_denied("treasury native transfer destination is not on the signer's treasury native allowlist"));
		}
		deny_over(network, TREASURY_NATIVE, "amount", amount, cap)
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

	fn check_allowlist(&self, network: Network, to_address: &str) -> Result<(), Status> {
		if !self.destination_allowlist.is_empty() && !self.allowlisted(network, to_address) {
			return Err(Status::permission_denied("treasury transfer destination is not on the signer's allowlist"));
		}
		Ok(())
	}

	/// USDT-allowlist membership; see [`listed`].
	fn allowlisted(&self, network: Network, address: &str) -> bool {
		listed(&self.destination_allowlist, network, address)
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

	fn policy(max: u64, allow: &[&str]) -> SignerPolicy {
		SignerPolicy {
			max_transfer_usdt: max,
			destination_allowlist: allow.iter().map(|s| (*s).to_owned()).collect(),
			..SignerPolicy::default()
		}
	}

	/// The wire's `token_contract` for a legitimate treasury transfer: the default pin on the
	/// contract-bearing rails, nothing on TON.
	fn usdt(network: Network) -> Option<&'static str> {
		match network {
			Network::Bep20 => Some("0x55d398326f99059fF775485246999027B3197955"),
			Network::Polygon => Some("0xc2132D05D31c914a87C6611C10748AEb04B58e8F"),
			Network::Trc20 => Some("TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t"),
			Network::Ton => None,
		}
	}

	fn denied(result: Result<(), Status>) -> Status {
		let status = result.expect_err("expected a refusal");
		assert_eq!(status.code(), Code::PermissionDenied, "{status:?}");
		status
	}

	#[test]
	fn unconfigured_policy_caps_a_payout_at_the_production_default() {
		let p = SignerPolicy::default();
		// No allowlist: any address. But never uncapped — 100 USDT is the unset posture.
		assert_eq!(p.max_transfer_usdt(), 100);
		assert_eq!(p.treasury_usdt_per_hour(), 1_000);
		assert!(p.check_treasury_transfer(Network::Bep20, usdt(Network::Bep20), "0xanything", 100 * CANONICAL_PER_USDT).is_ok());
		let status = denied(p.check_treasury_transfer(Network::Bep20, usdt(Network::Bep20), "0xanything", 100 * CANONICAL_PER_USDT + 1));
		assert!(status.message().contains("per-transfer cap of 100 USDT"), "{status:?}");
	}

	#[test]
	fn treasury_usdt_window_admits_up_to_the_cap_in_each_chain_precision() {
		let p = SignerPolicy::default();
		// 1_000 USDT per hour: 18 dp on BEP20, 6 dp on Tron/TON.
		let cap_bep20 = 1_000 * CANONICAL_PER_USDT;
		assert!(p.check_treasury_usdt_window(Network::Bep20, 0, cap_bep20).is_ok());
		assert!(p.check_treasury_usdt_window(Network::Bep20, cap_bep20 - 1, 1).is_ok());
		let status = denied(p.check_treasury_usdt_window(Network::Bep20, cap_bep20, 1));
		assert!(status.message().contains("treasury USDT window"), "{status:?}");
		assert!(p.check_treasury_usdt_window(Network::Trc20, 900_000_000, 100_000_000).is_ok());
		denied(p.check_treasury_usdt_window(Network::Trc20, 900_000_001, 100_000_000));
		assert!(p.check_treasury_usdt_window(Network::Ton, 0, 1_000_000_000).is_ok());
		denied(p.check_treasury_usdt_window(Network::Ton, 0, 1_000_000_001));
		let status = denied(p.check_treasury_usdt_window(Network::Polygon, u128::MAX, 1));
		assert!(status.message().contains("overflows"), "{status:?}");
	}

	#[test]
	fn usdt_caps_come_from_env_and_refuse_zero() {
		let p = SignerPolicy::from_lookup(&lookup(&[("SIGNER_MAX_TRANSFER_USDT", "250"), ("SIGNER_MAX_TREASURY_USDT_PER_HOUR", "2500")])).unwrap();
		assert_eq!(p.max_transfer_usdt(), 250);
		assert_eq!(p.treasury_usdt_per_hour(), 2_500);
		assert!(p.check_treasury_usdt_window(Network::Ton, 0, 2_500_000_000).is_ok());
		denied(p.check_treasury_usdt_window(Network::Ton, 0, 2_500_000_001));
		// Unset or empty is the default, never "off"; zero and garbage do not boot.
		assert_eq!(SignerPolicy::from_lookup(&lookup(&[("SIGNER_MAX_TRANSFER_USDT", "")])).unwrap().max_transfer_usdt(), 100);
		for name in ["SIGNER_MAX_TRANSFER_USDT", "SIGNER_MAX_TREASURY_USDT_PER_HOUR"] {
			for bad in ["0", "x", "-5", "1.5"] {
				let err = SignerPolicy::from_lookup(&lookup(&[(name, bad)])).expect_err(&format!("{name}={bad} must not boot"));
				assert!(err.to_string().contains(name), "{err}");
			}
		}
	}

	#[test]
	fn cap_is_scaled_to_each_chain_precision() {
		let p = policy(1000, &[]);
		// BEP20 USDT is 18-dp: 1000 USDT = 1000e18 base units.
		let cap_bep20 = 1000u128 * CANONICAL_PER_USDT;
		assert!(p.check_treasury_transfer(Network::Bep20, usdt(Network::Bep20), "0xto", cap_bep20).is_ok());
		assert!(p.check_treasury_transfer(Network::Bep20, usdt(Network::Bep20), "0xto", cap_bep20 + 1).is_err());
		// TRC20/TON USDT is 6-dp: 1000 USDT = 1_000_000_000 base units.
		assert!(p.check_treasury_transfer(Network::Trc20, usdt(Network::Trc20), "Tto", 1_000_000_000).is_ok());
		assert!(p.check_treasury_transfer(Network::Trc20, usdt(Network::Trc20), "Tto", 1_000_000_001).is_err());
		assert!(p.check_treasury_transfer(Network::Ton, usdt(Network::Ton), "EQto", 1_000_000_000).is_ok());
	}

	#[test]
	fn allowlist_pins_destinations_when_set() {
		let p = policy(1000, &["0xgood", "0xalsogood"]);
		assert!(p.check_treasury_transfer(Network::Bep20, usdt(Network::Bep20), "0xgood", 1).is_ok());
		assert!(p.check_treasury_transfer(Network::Bep20, usdt(Network::Bep20), "0xbad", 1).is_err());
	}

	#[test]
	fn treasury_native_is_refused_by_default_whatever_else_is_set() {
		// Allowlisted, capped, tiny: still refused — nothing but the opt-in enables the flow.
		let p = policy(1, &["0xgood"]);
		let status = denied(p.check_treasury_native_transfer(Network::Bep20, "0xgood", 1));
		assert!(status.message().contains("SIGNER_ALLOW_TREASURY_NATIVE"), "{status:?}");
		denied(SignerPolicy::default().check_treasury_native_transfer(Network::Ton, "anything", 0));
	}

	/// Opted in with `allow` as the NATIVE allowlist and the USDT allowlist empty.
	fn native_opted_in(allow: &[&str], caps: TreasuryNativeCaps) -> SignerPolicy {
		SignerPolicy {
			treasury_native: Some(caps),
			treasury_native_allowlist: allow.iter().map(|s| (*s).to_owned()).collect(),
			..policy(1000, &[])
		}
	}

	#[test]
	fn treasury_native_opted_in_needs_the_allowlist_and_the_rail_ceiling() {
		let caps = TreasuryNativeCaps {
			bep20_wei: Some(1_000),
			..TreasuryNativeCaps::default()
		};
		let p = native_opted_in(&[EIP55], caps.clone());
		assert!(p.check_treasury_native_transfer(Network::Bep20, &EIP55.to_ascii_lowercase(), 1_000).is_ok());
		let over = denied(p.check_treasury_native_transfer(Network::Bep20, EIP55, 1_001));
		assert!(over.message().contains("treasury native"), "{over:?}");
		let off_list = denied(p.check_treasury_native_transfer(Network::Bep20, OTHER_EVM, 1));
		assert!(off_list.message().contains("treasury native allowlist"), "{off_list:?}");
		// The lists do not read each other: a USDT payout to an address only the native list
		// names is off the (empty ⇒ anywhere) USDT list's business, and a native transfer to an
		// address only the USDT list names is refused.
		assert!(p.check_treasury_transfer(Network::Bep20, usdt(Network::Bep20), OTHER_EVM, 1).is_ok());
		let usdt_listed = SignerPolicy {
			destination_allowlist: vec![OTHER_EVM.to_owned()],
			..p.clone()
		};
		denied(usdt_listed.check_treasury_native_transfer(Network::Bep20, OTHER_EVM, 1));
		denied(usdt_listed.check_treasury_transfer(Network::Bep20, usdt(Network::Bep20), EIP55, 1));
		// A rail without a ceiling stays refused even though the flow is on.
		let no_cap = denied(p.check_treasury_native_transfer(Network::Polygon, EIP55, 1));
		assert!(no_cap.message().contains("SIGNER_MAX_TREASURY_NATIVE"), "{no_cap:?}");
		// An empty allowlist is never "anywhere" for this flow (loading refuses it; the check
		// does too, so a hand-built policy cannot reopen the hole).
		denied(native_opted_in(&[], caps).check_treasury_native_transfer(Network::Bep20, EIP55, 1));
	}

	#[test]
	fn treasury_native_opt_in_is_refused_at_boot_without_its_bounds() {
		let err = SignerPolicy::from_lookup(&lookup(&[("SIGNER_ALLOW_TREASURY_NATIVE", "true")])).unwrap_err();
		assert!(err.to_string().contains("SIGNER_TREASURY_NATIVE_ALLOWLIST"), "{err}");
		// The USDT allowlist does not satisfy the opt-in — it is the other list.
		let err = SignerPolicy::from_lookup(&lookup(&[("SIGNER_ALLOW_TREASURY_NATIVE", "true"), ("SIGNER_DESTINATION_ALLOWLIST", EIP55)])).unwrap_err();
		assert!(err.to_string().contains("SIGNER_TREASURY_NATIVE_ALLOWLIST"), "{err}");
		let err = SignerPolicy::from_lookup(&lookup(&[("SIGNER_ALLOW_TREASURY_NATIVE", "true"), ("SIGNER_TREASURY_NATIVE_ALLOWLIST", EIP55)])).unwrap_err();
		assert!(err.to_string().contains("SIGNER_MAX_TREASURY_NATIVE"), "{err}");
		let p = SignerPolicy::from_lookup(&lookup(&[
			("SIGNER_ALLOW_TREASURY_NATIVE", "TRUE"),
			("SIGNER_TREASURY_NATIVE_ALLOWLIST", &format!(" {EIP55} ,, ")),
			("SIGNER_MAX_TREASURY_NATIVE_TON", "5"),
		]))
		.unwrap();
		assert_eq!(p.treasury_native_allowlist_len(), 1);
		assert_eq!(p.allowlist_len(), 0);
		assert_eq!(
			p.treasury_native(),
			Some(&TreasuryNativeCaps {
				ton_nano: Some(5),
				..TreasuryNativeCaps::default()
			})
		);
		// Off, explicitly or by absence; anything else is a typo, not a choice.
		assert!(
			SignerPolicy::from_lookup(&lookup(&[("SIGNER_ALLOW_TREASURY_NATIVE", "false")]))
				.unwrap()
				.treasury_native()
				.is_none()
		);
		assert!(SignerPolicy::from_lookup(&lookup(&[("SIGNER_ALLOW_TREASURY_NATIVE", "")])).unwrap().treasury_native().is_none());
		assert!(SignerPolicy::from_lookup(&lookup(&[("SIGNER_ALLOW_TREASURY_NATIVE", "yes")])).is_err());
		// A zero ceiling is a typo too, even with the flow off.
		assert!(
			SignerPolicy::from_lookup(&lookup(&[("SIGNER_MAX_TREASURY_NATIVE_BEP20", "0")])).is_ok(),
			"ignored while the flow is off"
		);
		assert!(
			SignerPolicy::from_lookup(&lookup(&[
				("SIGNER_ALLOW_TREASURY_NATIVE", "true"),
				("SIGNER_TREASURY_NATIVE_ALLOWLIST", EIP55),
				("SIGNER_MAX_TREASURY_NATIVE_BEP20", "0"),
			]))
			.is_err()
		);
	}

	// === treasury: token pin ======================================================

	#[test]
	fn treasury_token_transfer_must_name_the_pinned_contract_in_any_rendering() {
		let p = SignerPolicy::default();
		let bep20 = usdt(Network::Bep20).unwrap();
		assert!(p.check_treasury_transfer(Network::Bep20, Some(&bep20.to_ascii_lowercase()), OTHER_EVM, 1).is_ok());
		let status = denied(p.check_treasury_transfer(Network::Bep20, Some(OTHER_EVM), OTHER_EVM, 1));
		assert!(status.message().contains("token_contract"), "{status:?}");
		// Polygon's pin is a different contract: BSC's USDT is not it.
		denied(p.check_treasury_transfer(Network::Polygon, Some(bep20), OTHER_EVM, 1));
		assert!(p.check_treasury_transfer(Network::Polygon, usdt(Network::Polygon), OTHER_EVM, 1).is_ok());
		denied(p.check_treasury_transfer(Network::Trc20, Some(TRON), TRON, 1));
		// The pin outranks the allowlist and the cap: a listed destination under the cap with the
		// wrong token is still refused.
		denied(policy(1000, &[OTHER_EVM]).check_treasury_transfer(Network::Bep20, Some(OTHER_EVM), OTHER_EVM, 1));
		// Presence mismatches are our bug, not a policy verdict.
		assert_eq!(p.check_treasury_transfer(Network::Ton, Some(OWN_BASE64), OWN_BASE64, 1).unwrap_err().code(), Code::Internal);
		assert_eq!(p.check_treasury_transfer(Network::Bep20, None, OTHER_EVM, 1).unwrap_err().code(), Code::Internal);
	}

	#[test]
	fn token_pins_come_from_env_and_must_parse_as_their_network() {
		assert_eq!(TokenPins::from_lookup(&lookup(&[])).unwrap(), TokenPins::default());
		let pins = TokenPins::from_lookup(&lookup(&[("SIGNER_USDT_CONTRACT_BEP20", OTHER_EVM), ("SIGNER_USDT_CONTRACT_TRC20", TRON)])).unwrap();
		assert_eq!(pins.pinned(Network::Bep20), Some(OTHER_EVM));
		assert_eq!(pins.pinned(Network::Trc20), Some(TRON));
		assert_eq!(pins.pinned(Network::Polygon), Some(TokenPins::default().polygon.as_str()));
		assert_eq!(pins.pinned(Network::Ton), None);
		// An override that is not an address of its network does not boot.
		for (name, bad) in [
			("SIGNER_USDT_CONTRACT_BEP20", TRON),
			("SIGNER_USDT_CONTRACT_POLYGON", "0x1234"),
			("SIGNER_USDT_CONTRACT_TRC20", OTHER_EVM),
			("SIGNER_USDT_CONTRACT_TRC20", &TRON.to_ascii_lowercase()),
		] {
			let err = TokenPins::from_lookup(&lookup(&[(name, bad)])).expect_err(&format!("{name}={bad} must not boot"));
			assert!(err.to_string().contains(name), "{err}");
		}
	}

	// === native spend window ======================================================

	#[test]
	fn native_spend_window_admits_up_to_the_cap_and_refuses_the_next_unit() {
		let p = SignerPolicy::default();
		let cap = 1_000_000_000_000_000_000;
		assert!(p.check_native_spend_window(Network::Bep20, 0, cap).is_ok());
		assert!(p.check_native_spend_window(Network::Bep20, cap - 1, 1).is_ok());
		let status = denied(p.check_native_spend_window(Network::Bep20, cap, 1));
		assert!(status.message().contains("native spend window"), "{status:?}");
		denied(p.check_native_spend_window(Network::Bep20, 0, cap + 1));
		// Per-rail caps: 50 POL, 2_000 TRX, 5 TON.
		assert!(p.check_native_spend_window(Network::Polygon, 0, 50_000_000_000_000_000_000).is_ok());
		denied(p.check_native_spend_window(Network::Polygon, 1, 50_000_000_000_000_000_000));
		assert!(p.check_native_spend_window(Network::Trc20, 1_900_000_000, 100_000_000).is_ok());
		denied(p.check_native_spend_window(Network::Trc20, 1_900_000_001, 100_000_000));
		assert!(p.check_native_spend_window(Network::Ton, 4_800_000_000, 200_000_000).is_ok());
		denied(p.check_native_spend_window(Network::Ton, 4_800_000_001, 200_000_000));
		// A sum that does not fit is refused as such, never compared after wrapping.
		let status = denied(p.check_native_spend_window(Network::Ton, u128::MAX, 1));
		assert!(status.message().contains("overflows"), "{status:?}");
	}

	#[test]
	fn evm_native_spend_is_fee_plus_value_and_refuses_overflow() {
		assert_eq!(evm_native_spend(Network::Bep20, 5 * GWEI, 21_000, 7).unwrap(), 5 * GWEI * 21_000 + 7);
		assert_eq!(evm_native_spend(Network::Bep20, 5 * GWEI, 60_000, 0).unwrap(), 5 * GWEI * 60_000);
		assert_eq!(evm_native_spend(Network::Bep20, u128::MAX, 2, 0).unwrap_err().code(), Code::PermissionDenied);
		assert_eq!(evm_native_spend(Network::Bep20, u128::MAX, 1, 1).unwrap_err().code(), Code::PermissionDenied);
	}

	#[test]
	fn native_spend_window_comes_from_env_and_refuses_zero() {
		assert_eq!(NativeSpendWindow::from_lookup(&lookup(&[])).unwrap(), NativeSpendWindow::default());
		let window = NativeSpendWindow::from_lookup(&lookup(&[
			("SIGNER_MAX_NATIVE_SPEND_PER_HOUR_BEP20", "1"),
			("SIGNER_MAX_NATIVE_SPEND_PER_HOUR_POLYGON", "2"),
			("SIGNER_MAX_NATIVE_SPEND_PER_HOUR_TRC20", "3"),
			("SIGNER_MAX_NATIVE_SPEND_PER_HOUR_TON", "340282366920938463463374607431768211455"),
		]))
		.unwrap();
		assert_eq!(
			window,
			NativeSpendWindow {
				bep20_wei: 1,
				polygon_wei: 2,
				trc20_sun: 3,
				ton_nano: u128::MAX,
			}
		);
		for name in [
			"SIGNER_MAX_NATIVE_SPEND_PER_HOUR_BEP20",
			"SIGNER_MAX_NATIVE_SPEND_PER_HOUR_POLYGON",
			"SIGNER_MAX_NATIVE_SPEND_PER_HOUR_TRC20",
			"SIGNER_MAX_NATIVE_SPEND_PER_HOUR_TON",
		] {
			for bad in ["0", "abc", "-5", "1.5"] {
				let err = NativeSpendWindow::from_lookup(&lookup(&[(name, bad)])).expect_err(&format!("{name}={bad} must not boot"));
				assert!(err.to_string().contains(name), "{err}");
			}
		}
		assert_eq!(SPEND_WINDOW, Duration::from_secs(3600));
	}

	// === tron: frozen by default ==================================================

	#[test]
	fn tron_signing_is_off_unless_the_flag_says_true() {
		let off = SignerPolicy::from_lookup(&lookup(&[])).unwrap();
		assert!(!off.tron_signing_enabled());
		let status = denied(off.check_tron_signing());
		assert!(status.message().contains("SIGNER_TRON_SIGNING_ENABLED"), "{status:?}");
		let on = SignerPolicy::from_lookup(&lookup(&[("SIGNER_TRON_SIGNING_ENABLED", " True ")])).unwrap();
		assert!(on.tron_signing_enabled());
		assert!(on.check_tron_signing().is_ok());
		assert!(!SignerPolicy::from_lookup(&lookup(&[("SIGNER_TRON_SIGNING_ENABLED", "false")])).unwrap().tron_signing_enabled());
		assert!(SignerPolicy::from_lookup(&lookup(&[("SIGNER_TRON_SIGNING_ENABLED", "1")])).is_err());
	}

	// === allowlist: rendering-aware membership ===================================

	const EIP55: &str = "0x7E5F4552091A69125d5DfCb7b8C2659029395Bdf";
	const OTHER_EVM: &str = "0x024da544a76714a3812096e9ef84d40b2c8863e8";
	const TRON: &str = "TJRabPrwbZy45sbavfcjinPJC18kjpRTv8";

	#[test]
	fn allowlist_membership_ignores_eip55_casing_on_evm() {
		let p = policy(1000, &[EIP55]);
		assert!(p.check_treasury_transfer(Network::Bep20, usdt(Network::Bep20), &EIP55.to_ascii_lowercase(), 1).is_ok());
		assert!(
			p.check_treasury_transfer(Network::Polygon, usdt(Network::Polygon), &EIP55.to_ascii_uppercase().replace("0X", "0x"), 1)
				.is_ok()
		);
		// The list may be spelled lowercase while the hub sends EIP-55.
		assert!(
			policy(1000, &[&EIP55.to_ascii_lowercase()])
				.check_treasury_transfer(Network::Bep20, usdt(Network::Bep20), EIP55, 1)
				.is_ok()
		);
		denied(p.check_treasury_transfer(Network::Bep20, usdt(Network::Bep20), OTHER_EVM, 1));
	}

	#[test]
	fn allowlist_membership_spans_ton_raw_and_base64_but_keeps_tron_case_sensitive() {
		let raw = own_raw();
		assert!(policy(1000, &[&raw]).check_treasury_transfer(Network::Ton, usdt(Network::Ton), OWN_BASE64, 1).is_ok());
		assert!(policy(1000, &[OWN_BASE64]).check_treasury_transfer(Network::Ton, usdt(Network::Ton), &raw, 1).is_ok());
		denied(policy(1000, &[OWN_BASE64]).check_treasury_transfer(Network::Ton, usdt(Network::Ton), FOREIGN, 1));
		// Tron's Base58Check is case-sensitive: a re-cased string is a different (invalid) address.
		assert!(policy(1000, &[TRON]).check_treasury_transfer(Network::Trc20, usdt(Network::Trc20), TRON, 1).is_ok());
		denied(policy(1000, &[TRON]).check_treasury_transfer(Network::Trc20, usdt(Network::Trc20), &TRON.to_ascii_lowercase(), 1));
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
		let p = policy(1000, &["0xgood"]);
		// On the allowlist but over the cap → denied.
		assert!(p.check_treasury_transfer(Network::Bep20, usdt(Network::Bep20), "0xgood", 2000 * CANONICAL_PER_USDT).is_err());
		// Under the cap but off the allowlist → denied.
		assert!(p.check_treasury_transfer(Network::Bep20, usdt(Network::Bep20), "0xother", 1).is_err());
		// Under the cap and on the allowlist → allowed.
		assert!(p.check_treasury_transfer(Network::Bep20, usdt(Network::Bep20), "0xgood", 500 * CANONICAL_PER_USDT).is_ok());
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
		let p = policy(1000, &["EQsomeone_else"]);
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
		let status = denied(policy(1000, &["EQsomeone_else"]).check_treasury_response_destination(&own, FOREIGN));
		assert!(status.message().contains("response_destination"), "{status:?}");
		assert!(policy(1000, &[FOREIGN]).check_treasury_response_destination(&own, FOREIGN).is_ok());
		// Garbage is not the wallet's own address either.
		denied(policy(1000, &[FOREIGN]).check_treasury_response_destination(&own, "not-an-address"));
	}

	#[test]
	fn response_destination_must_be_the_own_wallet_without_an_allowlist() {
		// The default (prod) posture: no allowlist, yet the excess may only return to the wallet.
		let p = policy(1000, &[]);
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
	fn jetton_wallet_pin_admits_only_itself_in_either_rendering() {
		for source in [JettonPinSource::Operator, JettonPinSource::FirstUse] {
			assert!(check_jetton_wallet_pin(JETTON_WALLET_RAW, JETTON_WALLET_RAW, source).is_ok());
			assert!(check_jetton_wallet_pin(JETTON_WALLET_RAW, &jetton_wallet_base64(), source).is_ok());
			assert!(check_jetton_wallet_pin(&jetton_wallet_base64(), JETTON_WALLET_RAW, source).is_ok());
			let status = denied(check_jetton_wallet_pin(JETTON_WALLET_RAW, FOREIGN, source));
			assert!(status.message().contains("our_jetton_wallet"), "{status:?}");
			denied(check_jetton_wallet_pin(JETTON_WALLET_RAW, "garbage", source));
		}
		// The wording names the pin to look at.
		assert!(
			denied(check_jetton_wallet_pin(JETTON_WALLET_RAW, FOREIGN, JettonPinSource::Operator))
				.message()
				.contains("SIGNER_TON_TREASURY_JETTON_WALLET")
		);
		assert!(
			denied(check_jetton_wallet_pin(JETTON_WALLET_RAW, FOREIGN, JettonPinSource::FirstUse))
				.message()
				.contains("first use")
		);
	}

	#[test]
	fn operator_pin_is_exposed_only_when_set() {
		let p = SignerPolicy {
			ton_treasury_jetton_wallet: Some(JETTON_WALLET_RAW.to_owned()),
			..policy(1000, &[FOREIGN])
		};
		assert!(p.treasury_jetton_wallet_pinned());
		assert_eq!(p.treasury_jetton_wallet(), Some(JETTON_WALLET_RAW));
		// The allowlist has no say over the jetton wallet: unpinned is unpinned, listed or not.
		assert_eq!(policy(1000, &[JETTON_WALLET_RAW]).treasury_jetton_wallet(), None);
	}

	// === policy: env parsing ===================================================

	#[test]
	fn policy_from_lookup_reads_cap_allowlist_and_pin() {
		let p = SignerPolicy::from_lookup(&lookup(&[])).unwrap();
		assert_eq!(p.max_transfer_usdt(), 100);
		assert_eq!(p.allowlist_len(), 0);
		assert!(!p.treasury_jetton_wallet_pinned());

		let p = SignerPolicy::from_lookup(&lookup(&[
			("SIGNER_MAX_TRANSFER_USDT", "500"),
			("SIGNER_DESTINATION_ALLOWLIST", " 0xa , 0xb,, "),
			("SIGNER_TON_TREASURY_JETTON_WALLET", JETTON_WALLET_RAW),
		]))
		.unwrap();
		assert_eq!(p.max_transfer_usdt(), 500);
		assert_eq!(p.allowlist_len(), 2);
		assert!(p.treasury_jetton_wallet_pinned());
		assert!(check_jetton_wallet_pin(p.treasury_jetton_wallet().unwrap(), &jetton_wallet_base64(), JettonPinSource::Operator).is_ok());

		// An empty pin is unset; a pin that is not a TON address does not boot.
		assert!(
			!SignerPolicy::from_lookup(&lookup(&[("SIGNER_TON_TREASURY_JETTON_WALLET", "")]))
				.unwrap()
				.treasury_jetton_wallet_pinned()
		);
		let err = SignerPolicy::from_lookup(&lookup(&[("SIGNER_TON_TREASURY_JETTON_WALLET", "not-an-address")])).unwrap_err();
		assert!(err.to_string().contains("SIGNER_TON_TREASURY_JETTON_WALLET"), "{err}");
	}
}
