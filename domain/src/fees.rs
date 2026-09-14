//! `fees` bounded context — the fund's management + performance fee ("2 and 20").
//!
//! Two charges sit on a holding, and they answer different questions:
//!
//! - **Management, 2% p.a.** — rent on the capital the investor has parked in the
//!   product. It accrues *continuously* with elapsed time and is charged on every
//!   assessment, whether the fund made money or not. Its base is a
//!   [`ManagementBasis`]: the **invested capital** (the position's cost basis — the
//!   "static money" that actually went in) by default, or the **market value**
//!   (`units × NAV`, the hedge-fund convention). Invested capital is the house
//!   default because it is the number the investor agreed to: it does not swell with
//!   a mark the manager posted, which keeps the fee the manager earns independent of
//!   the input the manager supplies.
//! - **Performance, 20% of the gain** — a share of profit above a **per-investor
//!   high-water mark**, crystallized at the end of a [`CrystallizationPeriod`]
//!   (annual by default) and again on the day an investor redeems. Optionally the
//!   gain must first clear a **hurdle** accruing at `hurdle_bps` p.a. over the mark.
//!
//! ## Why the mark is per investor, not per fund
//!
//! A fund-level mark (what an on-chain vault must use, because a share token cannot
//! remember who bought when) mutualizes the fee: an investor who subscribes after a
//! drawdown rides the recovery fee-free, while one who subscribed at the top pays on
//! gains that only restored their own loss. The industry's two fixes are *series
//! accounting* (a new share class per dealing day) and *equalization* (per-investor
//! credits and debits against one class). This context takes the third road the ledger
//! here makes cheap: the position projection already exists per `(user, service)`, so
//! the mark simply lives on it. Every investor's fee is measured against their own
//! entry price, and nobody subsidises anybody.
//!
//! ## Why the fee is taken in units, never in cash
//!
//! A charge is settled by **clawing back units** — `Dr FeeShares / Cr UserShares` on
//! the Share ledger — never by moving USDT. Three properties follow, and they are the
//! reason for the design:
//!
//! 1. **No chain fee, ever.** Nothing leaves custody when a fee is charged, so the
//!    fund pays no gas per investor per period. The manager converts an accumulated
//!    unit balance to cash *once*, in bulk, via [`FeeSettlement`].
//! 2. **An investor can never be pushed negative.** The cash claim is not touched at
//!    all, and the clawback is capped by the units actually held — with TigerBeetle's
//!    `credits_must_not_exceed_debits` flag on `UserShares` as the ledger backstop
//!    underneath the application cap.
//! 3. **Other investors are untouched.** `SharesOutstanding` does not move, so NAV per
//!    unit is unchanged. The charge is a transfer *between holders*, not a dilution of
//!    everyone — which is what makes the per-investor mark honest.
//!
//! Whatever cannot be collected (the holder's units are locked by a queued redemption,
//! or the residue below one base-unit of share) is carried as
//! [`PositionSnapshot::debt`] and collected on the next assessment. It is never written
//! off and never turns into a negative balance.
//!
//! Pure and wasm-safe: no clock, no I/O, ids minted by the application layer.

use ev::architecture::{AggregateRoot, DomainEvent, EmitsEvents, Entity, Id};
use serde::{Deserialize, Serialize};

use crate::{
	balance::ServiceId,
	error::DomainError,
	money::{Nav, Shares, Usdt},
	push_field,
	users::UserId,
};

/// Basis-point denominator: `200 bps = 2%`, `2000 bps = 20%`.
pub const BPS: u128 = 10_000;
/// The accrual year. A flat 365 days — never a calendar year — so an accrual is a pure
/// function of elapsed seconds and a leap year cannot make one period quietly cheaper
/// than the next.
pub const SECONDS_PER_YEAR: u128 = 365 * 24 * 60 * 60;

/// The house rule: 2% management.
pub const HOUSE_MANAGEMENT_BPS: u32 = 200;
/// The house rule: 20% performance.
pub const HOUSE_PERFORMANCE_BPS: u32 = 2_000;

/// The most a management rate can ever be set to: 5% p.a. Beyond the house rule's 2% by a
/// margin a real mandate might need, and nowhere near a figure that could empty a holding
/// in a year. Mirrored by a `CHECK` on `fee_policies`, so no write path — not even a direct
/// SQL one — can put a higher number where the sweeper reads it.
pub const MAX_MANAGEMENT_BPS: u32 = 500;
/// The most a performance rate can ever be set to: 50% of the gain. Only the gain above the
/// investor's own mark is at stake, so the ceiling is generous; above half of a gain a
/// "fee" is a different word for "confiscation".
pub const MAX_PERFORMANCE_BPS: u32 = 5_000;

/// The shortest notice a unit holder gets before a change of terms binds them: 24 hours,
/// measured from the moment the change is SCHEDULED (for a consilium-gated change, that is
/// the moment the owners carry it, not the moment it was proposed). A fund with no holders
/// has nobody to warn, and the change may take effect at once.
pub const MIN_NOTICE_SECS: i64 = 24 * 60 * 60;

/// What the management fee is charged on.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagementBasis {
	/// The position's cost basis — the cash the investor actually put in, unchanged by
	/// any mark. The default: it is the "static money", and it keeps the fee the
	/// operator earns independent of the AUM the operator posts.
	InvestedCapital,
	/// `units × NAV` — the hedge-fund convention. Moves with the mark, so an operator
	/// posting a higher AUM immediately raises their own fee; use only with an
	/// independent valuation source.
	MarketValue,
}

impl ManagementBasis {
	pub const fn as_str(self) -> &'static str {
		match self {
			Self::InvestedCapital => "invested_capital",
			Self::MarketValue => "market_value",
		}
	}

	pub fn parse(raw: &str) -> Result<Self, DomainError> {
		match raw {
			"invested_capital" => Ok(Self::InvestedCapital),
			"market_value" => Ok(Self::MarketValue),
			other => Err(DomainError::Validation(format!("unknown management basis: {other}"))),
		}
	}
}

/// How often the performance fee crystallizes — the moment a gain above the mark stops
/// being paper and becomes a charge, and the mark ratchets up.
///
/// The frequency is a *price*, not a detail: crystallizing quarterly instead of annually
/// measurably raises what an investor pays over a fund's life, because each reset locks
/// in gains that a later loss can no longer claw back. Annual is the default for exactly
/// that reason.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CrystallizationPeriod {
	Monthly,
	Quarterly,
	SemiAnnual,
	Annual,
}

impl CrystallizationPeriod {
	/// The period in seconds, on the same flat-365 year as [`SECONDS_PER_YEAR`].
	pub const fn seconds(self) -> u128 {
		match self {
			Self::Monthly => SECONDS_PER_YEAR / 12,
			Self::Quarterly => SECONDS_PER_YEAR / 4,
			Self::SemiAnnual => SECONDS_PER_YEAR / 2,
			Self::Annual => SECONDS_PER_YEAR,
		}
	}

	pub const fn as_str(self) -> &'static str {
		match self {
			Self::Monthly => "monthly",
			Self::Quarterly => "quarterly",
			Self::SemiAnnual => "semi_annual",
			Self::Annual => "annual",
		}
	}

	pub fn parse(raw: &str) -> Result<Self, DomainError> {
		match raw {
			"monthly" => Ok(Self::Monthly),
			"quarterly" => Ok(Self::Quarterly),
			"semi_annual" => Ok(Self::SemiAnnual),
			"annual" => Ok(Self::Annual),
			other => Err(DomainError::Validation(format!("unknown crystallization period: {other}"))),
		}
	}
}

/// One fund's fee terms, registered per [`ServiceId`] alongside its allocation. A
/// product with no policy charges nothing, so the fee is opt-in per product and can
/// never appear by accident on a fund whose prospectus did not promise it.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FeePolicy {
	management_bps: u32,
	performance_bps: u32,
	hurdle_bps: u32,
	basis: ManagementBasis,
	crystallization: CrystallizationPeriod,
}

impl FeePolicy {
	/// 2 and 20, annual crystallization, no hurdle, charged on invested capital.
	pub const HOUSE: FeePolicy = FeePolicy {
		management_bps: HOUSE_MANAGEMENT_BPS,
		performance_bps: HOUSE_PERFORMANCE_BPS,
		hurdle_bps: 0,
		basis: ManagementBasis::InvestedCapital,
		crystallization: CrystallizationPeriod::Annual,
	};
	/// The terms of a product with NO policy row: nothing is charged. This is what a first
	/// policy is measured against when deciding whether it tightens the terms.
	pub const NONE: FeePolicy = FeePolicy {
		management_bps: 0,
		performance_bps: 0,
		hurdle_bps: 0,
		basis: ManagementBasis::InvestedCapital,
		crystallization: CrystallizationPeriod::Annual,
	};

	/// Validate and build. The two fee rates are capped well below 100%
	/// ([`MAX_MANAGEMENT_BPS`], [`MAX_PERFORMANCE_BPS`]): a rate near the thing it is charged
	/// on is never a term, and a cap in the constructor means no code path can even
	/// represent one. The hurdle is capped at 100% only — it moves in the investor's favour,
	/// so a high one is generous rather than dangerous.
	pub fn new(management_bps: u32, performance_bps: u32, hurdle_bps: u32, basis: ManagementBasis, crystallization: CrystallizationPeriod) -> Result<Self, DomainError> {
		if management_bps > MAX_MANAGEMENT_BPS {
			return Err(DomainError::Validation(format!("management rate must be 0..={MAX_MANAGEMENT_BPS} bps")));
		}
		if performance_bps > MAX_PERFORMANCE_BPS {
			return Err(DomainError::Validation(format!("performance rate must be 0..={MAX_PERFORMANCE_BPS} bps")));
		}
		if u128::from(hurdle_bps) > BPS {
			return Err(DomainError::Validation(format!("hurdle rate must be 0..={BPS} bps")));
		}
		Ok(Self {
			management_bps,
			performance_bps,
			hurdle_bps,
			basis,
			crystallization,
		})
	}

	pub const fn management_bps(&self) -> u32 {
		self.management_bps
	}

	pub const fn performance_bps(&self) -> u32 {
		self.performance_bps
	}

	pub const fn hurdle_bps(&self) -> u32 {
		self.hurdle_bps
	}

	pub const fn basis(&self) -> ManagementBasis {
		self.basis
	}

	pub const fn crystallization(&self) -> CrystallizationPeriod {
		self.crystallization
	}

	/// Whether this policy charges anything at all.
	pub const fn is_zero(&self) -> bool {
		self.management_bps == 0 && self.performance_bps == 0
	}

	/// Whether these terms sit inside the HOUSE ENVELOPE: at most the house 2-and-20, charged
	/// on invested capital, crystallized annually. The hurdle is free — it only ever lowers
	/// the fee. A change that lands inside the envelope is one the prospectus already
	/// promised, and a single administrator may schedule it; one that leaves the envelope is
	/// a new bargain, and needs the owners.
	pub const fn within_house_envelope(&self) -> bool {
		self.management_bps <= HOUSE_MANAGEMENT_BPS
			&& self.performance_bps <= HOUSE_PERFORMANCE_BPS
			&& matches!(self.basis, ManagementBasis::InvestedCapital)
			&& matches!(self.crystallization, CrystallizationPeriod::Annual)
	}

	/// Whether moving from `current` to these terms makes ANY leg dearer for the investor:
	/// either rate up, the hurdle down, the basis moved onto the mark, or crystallization
	/// made more frequent. Each is judged on its own — a change that raises one rate while
	/// lowering another still tightens, because the investor it hurts is not the investor
	/// it helps.
	pub fn tightens_from(&self, current: &FeePolicy) -> bool {
		self.management_bps > current.management_bps
			|| self.performance_bps > current.performance_bps
			|| self.hurdle_bps < current.hurdle_bps
			|| (current.basis == ManagementBasis::InvestedCapital && self.basis == ManagementBasis::MarketValue)
			|| self.crystallization.seconds() < current.crystallization.seconds()
	}
}

/// Who has to agree before a change of terms takes effect.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeRequirement {
	/// One `AllocationManage` holder is enough.
	Admin,
	/// The owners' quorum — see `docs/CONSILIUM.md`.
	OwnerConsilium,
}

impl ChangeRequirement {
	pub const fn as_str(self) -> &'static str {
		match self {
			Self::Admin => "admin",
			Self::OwnerConsilium => "owner_consilium",
		}
	}

	pub fn parse(raw: &str) -> Result<Self, DomainError> {
		match raw {
			"admin" => Ok(Self::Admin),
			"owner_consilium" => Ok(Self::OwnerConsilium),
			other => Err(DomainError::Validation(format!("unknown fee-policy change requirement: {other}"))),
		}
	}
}

/// The one rule deciding who approves a change of terms: the owners, exactly when the
/// change TIGHTENS the terms AND lands OUTSIDE the house envelope. Everything else — a
/// loosening, or a tightening that stays within what the prospectus promised — is an
/// administrator's call. `None` for `current` is a product with no policy yet, which
/// charges nothing, so any first policy with a positive rate tightens.
pub fn requirement_for(current: Option<&FeePolicy>, next: &FeePolicy) -> ChangeRequirement {
	let current = current.copied().unwrap_or(FeePolicy::NONE);
	if next.tightens_from(&current) && !next.within_house_envelope() {
		ChangeRequirement::OwnerConsilium
	} else {
		ChangeRequirement::Admin
	}
}

/// When a change scheduled at `scheduled_at` may bind the holders: never before the
/// operator's own `requested_effective_from`, and — while anyone actually holds units —
/// never before [`MIN_NOTICE_SECS`] have passed. A request earlier than the minimum is
/// lifted to it rather than refused: the operator asked for "as soon as allowed".
pub fn earliest_effective_from(scheduled_at: i64, requested_effective_from: i64, has_holders: bool) -> i64 {
	let floor = if has_holders { scheduled_at.saturating_add(MIN_NOTICE_SECS) } else { scheduled_at };
	requested_effective_from.max(floor)
}

/// A unique fee-policy-change id (UUID). Minted by the application layer.
pub type FeePolicyChangeId = Id<FeePolicyChangeTag>;
/// Phantom tag making [`FeePolicyChangeId`] a distinct, incompatible identity type.
pub struct FeePolicyChangeTag;

/// Where a change of terms stands.
///
/// `awaiting_consilium → scheduled → active → superseded` is the life of a change that
/// needs the owners; an administrator's change starts at `scheduled`. `rejected` is the
/// consilium's refusal, expiry, withdrawal or voiding; `cancelled` is an administrator
/// withdrawing a scheduled change before it took effect.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FeePolicyChangeState {
	AwaitingConsilium,
	Scheduled,
	Active,
	Superseded,
	Rejected,
	Cancelled,
}

impl FeePolicyChangeState {
	pub const fn as_str(self) -> &'static str {
		match self {
			Self::AwaitingConsilium => "awaiting_consilium",
			Self::Scheduled => "scheduled",
			Self::Active => "active",
			Self::Superseded => "superseded",
			Self::Rejected => "rejected",
			Self::Cancelled => "cancelled",
		}
	}

	pub fn parse(raw: &str) -> Result<Self, DomainError> {
		match raw {
			"awaiting_consilium" => Ok(Self::AwaitingConsilium),
			"scheduled" => Ok(Self::Scheduled),
			"active" => Ok(Self::Active),
			"superseded" => Ok(Self::Superseded),
			"rejected" => Ok(Self::Rejected),
			"cancelled" => Ok(Self::Cancelled),
			other => Err(DomainError::Validation(format!("unknown fee-policy change state: {other}"))),
		}
	}

	/// Whether the change is still on its way to taking effect. At most one such change
	/// exists per product (`fee_policy_changes_single_pending_idx`).
	pub const fn is_pending(self) -> bool {
		matches!(self, Self::AwaitingConsilium | Self::Scheduled)
	}
}

/// The immutable subject of a fee-policy consilium: WHICH change, over WHICH product, from
/// WHAT terms to WHAT terms, binding no earlier than WHEN. The change's id is inside the
/// hashed subject, so an approval of one change is never a valid signature over another
/// that happens to name the same terms.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FeePolicySubject {
	pub change_id: FeePolicyChangeId,
	pub service: ServiceId,
	/// The terms in force when the change was proposed ([`FeePolicy::NONE`] for a product
	/// with no policy) — what the owners are told they are moving AWAY from.
	pub from: FeePolicy,
	pub to: FeePolicy,
	/// The operator's requested effective moment (unix seconds; `0` = as soon as allowed).
	/// The actual moment is fixed when the change is scheduled, which for this kind is the
	/// moment the owners carry it, and is never earlier than this.
	pub requested_effective_from: i64,
}

impl FeePolicySubject {
	/// FROZEN, for the reason `RevenuePayoutTerms::DOMAIN` is: every stored `payload_hash`
	/// over a fee-policy change opens with these bytes.
	pub const DOMAIN: &'static [u8] = b"banking.v1.FeePolicySubject\x00";

	/// The bytes the payload hash is taken over — fixed field order, every variable-length
	/// part length-prefixed, both policies encoded leg by leg.
	pub fn canonical_bytes(&self) -> Vec<u8> {
		let mut out = Vec::with_capacity(Self::DOMAIN.len() + 160);
		out.extend_from_slice(Self::DOMAIN);
		push_field(&mut out, self.change_id.raw().as_bytes());
		push_field(&mut out, self.service.as_str().as_bytes());
		for policy in [&self.from, &self.to] {
			out.extend_from_slice(&policy.management_bps.to_be_bytes());
			out.extend_from_slice(&policy.performance_bps.to_be_bytes());
			out.extend_from_slice(&policy.hurdle_bps.to_be_bytes());
			push_field(&mut out, policy.basis.as_str().as_bytes());
			push_field(&mut out, policy.crystallization.as_str().as_bytes());
		}
		out.extend_from_slice(&self.requested_effective_from.to_be_bytes());
		out
	}
}

/// Everything an assessment needs to know about one holding, read at assessment time.
/// `units` is the **available** unit balance (posted minus anything a queued redemption
/// has locked) — the clawback is capped by it, so a locked holding defers rather than
/// fails.
#[derive(Clone, Copy, Debug)]
pub struct PositionSnapshot {
	pub units: Shares,
	/// Net cash invested (average cost) — the invested-capital base.
	pub cost_basis: Usdt,
	/// The NAV this investor last crystallized at (or entered at). Their own mark.
	pub high_water_mark: Nav,
	/// Fee assessed but not yet collected, carried from earlier assessments.
	pub debt: Usdt,
	/// Unix seconds of the last management accrual — the elapsed-time clock.
	pub accrued_at_unix: i64,
	/// Unix seconds of the last performance crystallization — the period clock.
	pub crystallized_at_unix: i64,
}

/// Why an assessment is running. The two differ in *which units* the performance fee
/// covers and whether the mark moves.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "trigger", rename_all = "snake_case")]
pub enum Trigger {
	/// The scheduled sweep. Performance crystallizes only once the period has actually
	/// elapsed, covers the whole holding, and ratchets the mark up to the current NAV.
	Period,
	/// An investor is redeeming. Only the units leaving crystallize, priced on the
	/// redemption day as if it were a period end — the standard treatment, and the one
	/// that stops an exit from being a way to walk away from an accrued fee. The mark is
	/// left where it is: the units that stay have not crystallized.
	Redemption { units: Shares },
}

impl Trigger {
	pub const fn as_str(self) -> &'static str {
		match self {
			Self::Period => "period",
			Self::Redemption { .. } => "redemption",
		}
	}
}

/// The outcome of assessing one holding: what is owed, what could be taken now, and
/// where the mark and the debt land.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FeeCharge {
	/// Time-based fee for the elapsed period. Always charged.
	pub management: Usdt,
	/// Performance fee actually crystallized by this assessment (zero when the period
	/// has not elapsed).
	pub performance: Usdt,
	/// What the performance fee *would* be if it crystallized right now — the figure to
	/// disclose so a position's value can be shown net of accrued fees. Never charged on
	/// its own.
	pub performance_accrued: Usdt,
	/// Uncollected fee carried into this assessment.
	pub debt_opening: Usdt,
	/// `management + performance + debt_opening`.
	pub due: Usdt,
	/// Units clawed back — `min(due / NAV, units held)`.
	pub charged_units: Shares,
	/// The cash value of `charged_units` at this NAV.
	pub charged_cash: Usdt,
	/// `due − charged_cash`, carried to the next assessment.
	pub debt_carried: Usdt,
	/// The mark to store after this assessment.
	pub high_water_mark: Nav,
	/// Whether the performance fee crystallized (and so whether the period clock resets).
	pub crystallized: bool,
}

impl FeeCharge {
	/// Nothing was collectable — the caller should persist nothing and leave every clock
	/// where it is, so the accrual simply continues into the next sweep.
	pub fn is_empty(&self) -> bool {
		self.charged_units.is_zero()
	}
}

/// Assess one holding against `policy` at `nav`.
///
/// Order matters and follows the industry convention: **management first, performance on
/// what is left**. Charging both against the same pre-fee unit count would take a
/// performance cut of capital the management fee has already claimed.
///
/// Nothing here reads a clock or a store; `now_unix` and the snapshot are supplied.
pub fn assess(policy: &FeePolicy, snapshot: &PositionSnapshot, nav: Nav, trigger: Trigger, now_unix: i64) -> Result<FeeCharge, DomainError> {
	if nav.is_zero() {
		return Err(DomainError::Validation("cannot assess fees at a zero nav".into()));
	}

	let management = management_due(policy, snapshot, nav, now_unix)?;
	// The performance base is net of the units the management fee just consumed.
	let management_units = Shares::from_cash(management, nav)?;
	let units_net = snapshot.units.checked_sub(management_units).unwrap_or(Shares::ZERO);

	let floor_nav = hurdle_floor(policy, snapshot, now_unix)?;
	let scope = match trigger {
		Trigger::Period => units_net,
		Trigger::Redemption { units } => units.min(units_net),
	};
	let performance_accrued = performance_due(policy, floor_nav, nav, scope)?;

	let crystallized = match trigger {
		Trigger::Period => u128::try_from(now_unix.saturating_sub(snapshot.crystallized_at_unix).max(0)).unwrap_or(0) >= policy.crystallization.seconds(),
		Trigger::Redemption { .. } => true,
	};
	let performance = if crystallized { performance_accrued } else { Usdt::ZERO };

	// Only a full-position crystallization ratchets the mark. A redemption crystallizes
	// the exiting units alone, so the units that remain keep the mark they entered on.
	let high_water_mark = match (crystallized, trigger) {
		(true, Trigger::Period) if nav > snapshot.high_water_mark => nav,
		_ => snapshot.high_water_mark,
	};

	let due = management
		.checked_add(performance)
		.and_then(|sum| sum.checked_add(snapshot.debt))
		.ok_or_else(|| DomainError::Validation("fee total overflows".into()))?;

	// Collect in units, capped by the holding. `from_cash` floors, so the sub-unit
	// residue falls into the carried debt rather than being rounded onto the investor.
	let charged_units = Shares::from_cash(due, nav)?.min(snapshot.units);
	let charged_cash = nav.value(charged_units)?;
	let debt_carried = due.checked_sub(charged_cash).unwrap_or(Usdt::ZERO);

	Ok(FeeCharge {
		management,
		performance,
		performance_accrued,
		debt_opening: snapshot.debt,
		due,
		charged_units,
		charged_cash,
		debt_carried,
		high_water_mark,
		crystallized,
	})
}

/// `base × rate × elapsed / year`, where `base` is the policy's chosen basis. Elapsed
/// time is clamped at zero: a clock that ran backwards (a re-assessment inside the same
/// second, a corrected timestamp) must never produce a negative — or, on unsigned
/// arithmetic, an enormous — fee.
///
/// Public because the management leg has to be settled at one more moment than an
/// assessment: the instant the *basis itself* changes. A top-up raises `cost_basis`
/// without the elapsed clock moving, so an assessment that ran afterwards would charge
/// the whole elapsed window on money that arrived at the end of it. The projection
/// writers call this first, carry the result into `fee_debt`, and reset the clock — see
/// `piggybank_core::infrastructure::fees::carry_accrual`.
pub fn management_due(policy: &FeePolicy, snapshot: &PositionSnapshot, nav: Nav, now_unix: i64) -> Result<Usdt, DomainError> {
	if policy.management_bps == 0 {
		return Ok(Usdt::ZERO);
	}
	let elapsed = u128::try_from(now_unix.saturating_sub(snapshot.accrued_at_unix).max(0)).unwrap_or(0);
	if elapsed == 0 {
		return Ok(Usdt::ZERO);
	}
	let base = match policy.basis {
		ManagementBasis::InvestedCapital => snapshot.cost_basis,
		ManagementBasis::MarketValue => nav.value(snapshot.units)?,
	};
	base.scale(u128::from(policy.management_bps) * elapsed, BPS * SECONDS_PER_YEAR)
}

/// The NAV a gain is measured from: the investor's mark, lifted by the hurdle rate
/// accrued over the time since their last crystallization. With `hurdle_bps == 0` this is
/// simply the mark.
fn hurdle_floor(policy: &FeePolicy, snapshot: &PositionSnapshot, now_unix: i64) -> Result<Nav, DomainError> {
	if policy.hurdle_bps == 0 {
		return Ok(snapshot.high_water_mark);
	}
	let elapsed = u128::try_from(now_unix.saturating_sub(snapshot.crystallized_at_unix).max(0)).unwrap_or(0);
	let lift = snapshot.high_water_mark.scale(u128::from(policy.hurdle_bps) * elapsed, BPS * SECONDS_PER_YEAR)?;
	snapshot
		.high_water_mark
		.checked_add(lift)
		.ok_or_else(|| DomainError::Validation("hurdle lifts the mark past the representable range".into()))
}

/// `rate × (nav − floor) × units`. Below the floor there is no gain, hence no fee — the
/// whole point of a high-water mark.
fn performance_due(policy: &FeePolicy, floor_nav: Nav, nav: Nav, units: Shares) -> Result<Usdt, DomainError> {
	if policy.performance_bps == 0 || units.is_zero() {
		return Ok(Usdt::ZERO);
	}
	let Some(gain_per_unit) = nav.checked_sub(floor_nav) else {
		return Ok(Usdt::ZERO);
	};
	let profit = gain_per_unit.value(units)?;
	profit.scale(u128::from(policy.performance_bps), BPS)
}

/// A unique fee-assessment id (UUID). Minted by the application layer.
pub type FeeAssessmentId = Id<FeeAssessmentTag>;
/// Phantom tag making [`FeeAssessmentId`] a distinct, incompatible identity type.
pub struct FeeAssessmentTag;

/// A unique fee-settlement id (UUID). Minted by the application layer.
pub type FeeSettlementId = Id<FeeSettlementTag>;
/// Phantom tag making [`FeeSettlementId`] a distinct, incompatible identity type.
pub struct FeeSettlementTag;

/// One charge against one holding — an immutable record, like a subscription. Built from
/// an [`assess`] result that actually collected something; an empty charge is not an
/// assessment and must not be recorded (see [`FeeCharge::is_empty`]).
#[derive(Clone, Debug)]
pub struct FeeAssessment {
	id: FeeAssessmentId,
	user: UserId,
	service: ServiceId,
	nav: Nav,
	trigger: Trigger,
	charge: FeeCharge,
	pending: Vec<FeeEvent>,
}

impl FeeAssessment {
	/// Record the charge, raising `Charged`. The relay claws the units back
	/// (`Dr FeeShares / Cr UserShares`).
	pub fn record(id: FeeAssessmentId, user: UserId, service: ServiceId, nav: Nav, trigger: Trigger, charge: FeeCharge) -> Result<Self, DomainError> {
		if charge.is_empty() {
			return Err(DomainError::Validation("nothing to charge".into()));
		}
		let mut assessment = Self {
			id,
			user,
			service: service.clone(),
			nav,
			trigger,
			charge,
			pending: Vec::new(),
		};
		assessment.pending.push(FeeEvent::Charged {
			assessment_id: id,
			user,
			service,
			units: charge.charged_units,
			nav,
			management: charge.management,
			performance: charge.performance,
			cash: charge.charged_cash,
		});
		Ok(assessment)
	}

	/// Reconstitute from the store. Raises no events.
	pub fn rehydrate(id: FeeAssessmentId, user: UserId, service: ServiceId, nav: Nav, trigger: Trigger, charge: FeeCharge) -> Self {
		Self {
			id,
			user,
			service,
			nav,
			trigger,
			charge,
			pending: Vec::new(),
		}
	}

	pub fn id(&self) -> FeeAssessmentId {
		self.id
	}

	pub fn user(&self) -> UserId {
		self.user
	}

	pub fn service(&self) -> &ServiceId {
		&self.service
	}

	pub fn nav(&self) -> Nav {
		self.nav
	}

	pub fn trigger(&self) -> Trigger {
		self.trigger
	}

	pub fn charge(&self) -> FeeCharge {
		self.charge
	}
}

impl Entity for FeeAssessment {
	type Id = FeeAssessmentId;

	fn id(&self) -> FeeAssessmentId {
		self.id
	}
}

impl AggregateRoot for FeeAssessment {
	const NAME: &'static str = "fee_assessment";
}

impl EmitsEvents for FeeAssessment {
	type Event = FeeEvent;

	fn drain_events(&mut self) -> Vec<FeeEvent> {
		core::mem::take(&mut self.pending)
	}
}

/// Converting accumulated fee **units** into fee **cash** — the one place a fee touches
/// the money plane, and the reason clawing back in units costs nothing per investor: the
/// manager does this once for a whole period's fees, not once per holder.
///
/// It is the mirror of a redemption settle: burn the fee units, then pay their value out
/// of the fund's claim into fee revenue. Burn-first, so a fund short of cash parks before
/// any units are destroyed.
#[derive(Clone, Debug)]
pub struct FeeSettlement {
	id: FeeSettlementId,
	service: ServiceId,
	units: Shares,
	nav: Nav,
	cash: Usdt,
	pending: Vec<FeeEvent>,
}

impl FeeSettlement {
	/// Convert `units` of accumulated fee shares at `nav`. Rejects a zero request and
	/// one whose value floors to nothing (TigerBeetle rejects a zero-amount transfer).
	pub fn record(id: FeeSettlementId, service: ServiceId, units: Shares, nav: Nav) -> Result<Self, DomainError> {
		if units.is_zero() {
			return Err(DomainError::Validation("fee settlement units must be positive".into()));
		}
		let cash = nav.value(units)?;
		if cash.is_zero() {
			return Err(DomainError::Validation("fee units are worth nothing at this nav".into()));
		}
		let mut settlement = Self {
			id,
			service: service.clone(),
			units,
			nav,
			cash,
			pending: Vec::new(),
		};
		settlement.pending.push(FeeEvent::SharesSettled {
			settlement_id: id,
			service,
			units,
			nav,
			cash,
		});
		Ok(settlement)
	}

	pub fn id(&self) -> FeeSettlementId {
		self.id
	}

	pub fn service(&self) -> &ServiceId {
		&self.service
	}

	pub fn units(&self) -> Shares {
		self.units
	}

	pub fn nav(&self) -> Nav {
		self.nav
	}

	pub fn cash(&self) -> Usdt {
		self.cash
	}
}

impl Entity for FeeSettlement {
	type Id = FeeSettlementId;

	fn id(&self) -> FeeSettlementId {
		self.id
	}
}

impl AggregateRoot for FeeSettlement {
	const NAME: &'static str = "fee_settlement";
}

impl EmitsEvents for FeeSettlement {
	type Event = FeeEvent;

	fn drain_events(&mut self) -> Vec<FeeEvent> {
		core::mem::take(&mut self.pending)
	}
}

/// Facts raised by the fee context. Both carry everything the relay needs to post their
/// legs with no extra read. Internally tagged so the stored JSON is self-describing.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FeeEvent {
	/// Units clawed back from a holder (relay: `Dr FeeShares / Cr UserShares`). `cash` is
	/// their value at `nav`, carried for the audit trail — no cash moves.
	Charged {
		assessment_id: FeeAssessmentId,
		user: UserId,
		service: ServiceId,
		units: Shares,
		nav: Nav,
		management: Usdt,
		performance: Usdt,
		cash: Usdt,
	},
	/// Accumulated fee units converted to cash (relay, burn-first: post
	/// `Dr SharesOutstanding / Cr FeeShares`, then `Dr ServiceClaim / Cr FeeRevenue`).
	SharesSettled {
		settlement_id: FeeSettlementId,
		service: ServiceId,
		units: Shares,
		nav: Nav,
		cash: Usdt,
	},
}

impl DomainEvent for FeeEvent {
	const KIND: &'static str = "fees";
}

#[cfg(test)]
mod tests {
	use super::*;

	const YEAR: i64 = (SECONDS_PER_YEAR) as i64;

	fn svc() -> ServiceId {
		ServiceId::parse("trading").unwrap()
	}

	fn usdt(raw: &str) -> Usdt {
		Usdt::parse_decimal(raw).unwrap()
	}

	fn shares(raw: &str) -> Shares {
		Shares::parse_decimal(raw).unwrap()
	}

	fn nav(raw: &str) -> Nav {
		Nav::parse_decimal(raw).unwrap()
	}

	/// 1000 USDT in at NAV 1.0 → 1000 units, mark 1.0, clocks at t=0.
	fn snapshot() -> PositionSnapshot {
		PositionSnapshot {
			units: shares("1000"),
			cost_basis: usdt("1000"),
			high_water_mark: nav("1"),
			debt: Usdt::ZERO,
			accrued_at_unix: 0,
			crystallized_at_unix: 0,
		}
	}

	#[test]
	fn a_flat_year_charges_exactly_two_percent_of_invested_capital() {
		// NAV unchanged at 1.0, so there is no gain: management only.
		let charge = assess(&FeePolicy::HOUSE, &snapshot(), nav("1"), Trigger::Period, YEAR).unwrap();
		assert_eq!(charge.management, usdt("20"));
		assert_eq!(charge.performance, Usdt::ZERO);
		assert_eq!(charge.due, usdt("20"));
		// Taken in units at NAV 1.0 — 20 units, nothing left owing.
		assert_eq!(charge.charged_units, shares("20"));
		assert_eq!(charge.debt_carried, Usdt::ZERO);
		// The period elapsed, so the mark ratchets — to the same 1.0, there being no gain.
		assert!(charge.crystallized);
		assert_eq!(charge.high_water_mark, nav("1"));
	}

	#[test]
	fn a_year_at_a_higher_nav_charges_two_and_twenty_and_lifts_the_mark() {
		// NAV 1.0 → 1.5 on 1000 units. Management: 2% of 1000 invested = 20 USDT, which
		// at NAV 1.5 costs 13.333… units, leaving 986.666… for the performance leg.
		// Performance: 20% × 0.5 × 986.666… = 98.666… USDT.
		let charge = assess(&FeePolicy::HOUSE, &snapshot(), nav("1.5"), Trigger::Period, YEAR).unwrap();
		assert_eq!(charge.management, usdt("20"));
		assert_eq!(charge.performance, usdt("98.666666666666666666"));
		assert_eq!(charge.due, usdt("118.666666666666666666"));
		assert!(charge.crystallized);
		assert_eq!(charge.high_water_mark, nav("1.5"));
		// Charged in units at 1.5, and the sub-unit residue is carried, never rounded up.
		assert_eq!(charge.charged_units, Shares::from_cash(charge.due, nav("1.5")).unwrap());
		assert!(charge.debt_carried < usdt("0.000000000000000002"));
	}

	#[test]
	fn management_is_deducted_before_performance_is_measured() {
		// Charging both on the same pre-fee unit count would take 20% of 0.5 × 1000 = 100.
		// Netting the management fee out first is what makes it 98.66… — the difference is
		// the performance cut on capital management already claimed.
		let charge = assess(&FeePolicy::HOUSE, &snapshot(), nav("1.5"), Trigger::Period, YEAR).unwrap();
		assert!(charge.performance < usdt("100"));
	}

	#[test]
	fn the_mark_stops_a_fee_on_a_mere_recovery() {
		// The investor crystallized at 2.0; the fund fell to 1.2 and clawed back to 1.8.
		// That is a 50% gain on the year — and not one base unit of performance fee.
		let mut snap = snapshot();
		snap.high_water_mark = nav("2");
		let policy = FeePolicy::new(0, HOUSE_PERFORMANCE_BPS, 0, ManagementBasis::InvestedCapital, CrystallizationPeriod::Annual).unwrap();
		let charge = assess(&policy, &snap, nav("1.8"), Trigger::Period, YEAR).unwrap();
		assert_eq!(charge.performance, Usdt::ZERO);
		assert!(charge.is_empty());
		// And the mark does not fall to meet the lower NAV.
		assert_eq!(charge.high_water_mark, nav("2"));
	}

	#[test]
	fn performance_accrues_visibly_but_only_charges_at_the_period_end() {
		// Half a year in, at a NAV well above the mark.
		let half = YEAR / 2;
		let charge = assess(&FeePolicy::HOUSE, &snapshot(), nav("1.5"), Trigger::Period, half).unwrap();
		assert!(!charge.crystallized);
		// Disclosed (so a position can be shown net of accrued fees) but not taken.
		assert!(charge.performance_accrued > Usdt::ZERO);
		assert_eq!(charge.performance, Usdt::ZERO);
		// Management is time-based, so half a year is half of 2%.
		assert_eq!(charge.management, usdt("10"));
		assert_eq!(charge.due, usdt("10"));
		// The mark does not move on a non-crystallizing assessment.
		assert_eq!(charge.high_water_mark, nav("1"));
	}

	#[test]
	fn a_redemption_crystallizes_only_the_units_leaving_and_leaves_the_mark() {
		// A quarter of the position exits mid-period at NAV 1.5.
		let quarter = YEAR / 4;
		let charge = assess(&FeePolicy::HOUSE, &snapshot(), nav("1.5"), Trigger::Redemption { units: shares("250") }, quarter).unwrap();
		assert!(charge.crystallized);
		// 20% × 0.5 gain × 250 units = 25 USDT — the whole position would have been ~100.
		assert_eq!(charge.performance, usdt("25"));
		// The mark stays: the units that remain have not crystallized anything.
		assert_eq!(charge.high_water_mark, nav("1"));
	}

	#[test]
	fn a_hurdle_must_be_cleared_before_any_performance_fee() {
		// 8% hurdle over the mark of 1.0 → the fee only bites above 1.08 after a year.
		let policy = FeePolicy::new(0, HOUSE_PERFORMANCE_BPS, 800, ManagementBasis::InvestedCapital, CrystallizationPeriod::Annual).unwrap();
		let under = assess(&policy, &snapshot(), nav("1.05"), Trigger::Period, YEAR).unwrap();
		assert_eq!(under.performance, Usdt::ZERO);
		// At 1.20 the fee is 20% of the excess over 1.08, not over 1.00.
		let over = assess(&policy, &snapshot(), nav("1.2"), Trigger::Period, YEAR).unwrap();
		assert_eq!(over.performance, usdt("24"));
	}

	#[test]
	fn a_locked_holding_defers_the_whole_charge_instead_of_failing() {
		// Every unit is reserved by a queued redemption, so `units` (available) is zero.
		let mut snap = snapshot();
		snap.units = Shares::ZERO;
		let charge = assess(&FeePolicy::HOUSE, &snap, nav("1"), Trigger::Period, YEAR).unwrap();
		assert_eq!(charge.due, usdt("20"));
		assert_eq!(charge.charged_units, Shares::ZERO);
		// Nothing is written off — it all carries to the next assessment.
		assert_eq!(charge.debt_carried, usdt("20"));
		assert!(charge.is_empty());
	}

	#[test]
	fn a_partly_collectable_charge_carries_only_the_shortfall() {
		// The holding covers 5 USDT of a 20 USDT year at NAV 1.0.
		let mut snap = snapshot();
		snap.units = shares("5");
		let charge = assess(&FeePolicy::HOUSE, &snap, nav("1"), Trigger::Period, YEAR).unwrap();
		assert_eq!(charge.charged_units, shares("5"));
		assert_eq!(charge.charged_cash, usdt("5"));
		assert_eq!(charge.debt_carried, usdt("15"));
	}

	#[test]
	fn carried_debt_is_collected_on_the_next_assessment() {
		// Opening debt of 15, plus another flat year of management on the same basis.
		let mut snap = snapshot();
		snap.debt = usdt("15");
		let charge = assess(&FeePolicy::HOUSE, &snap, nav("1"), Trigger::Period, YEAR).unwrap();
		assert_eq!(charge.debt_opening, usdt("15"));
		assert_eq!(charge.due, usdt("35"));
		assert_eq!(charge.charged_units, shares("35"));
		assert_eq!(charge.debt_carried, Usdt::ZERO);
	}

	#[test]
	fn a_clock_that_ran_backwards_charges_nothing_rather_than_everything() {
		let mut snap = snapshot();
		snap.accrued_at_unix = YEAR * 2;
		let charge = assess(&FeePolicy::HOUSE, &snap, nav("1"), Trigger::Period, YEAR).unwrap();
		assert_eq!(charge.management, Usdt::ZERO);
	}

	#[test]
	fn a_zero_nav_is_refused_rather_than_dividing_by_it() {
		assert!(assess(&FeePolicy::HOUSE, &snapshot(), Nav::from_base_units(0), Trigger::Period, YEAR).is_err());
	}

	#[test]
	fn market_value_basis_tracks_the_mark_while_invested_capital_does_not() {
		let market = FeePolicy::new(HOUSE_MANAGEMENT_BPS, 0, 0, ManagementBasis::MarketValue, CrystallizationPeriod::Annual).unwrap();
		let capital = FeePolicy::new(HOUSE_MANAGEMENT_BPS, 0, 0, ManagementBasis::InvestedCapital, CrystallizationPeriod::Annual).unwrap();
		// 1000 units at NAV 1.5 is 1500 of market value → 2% is 30, against 20 on the
		// 1000 that actually went in.
		assert_eq!(assess(&market, &snapshot(), nav("1.5"), Trigger::Period, YEAR).unwrap().management, usdt("30"));
		assert_eq!(assess(&capital, &snapshot(), nav("1.5"), Trigger::Period, YEAR).unwrap().management, usdt("20"));
	}

	#[test]
	fn crystallization_frequency_decides_when_a_gain_is_locked_in() {
		let quarterly = FeePolicy::new(0, HOUSE_PERFORMANCE_BPS, 0, ManagementBasis::InvestedCapital, CrystallizationPeriod::Quarterly).unwrap();
		let annual = FeePolicy::HOUSE;
		let quarter = YEAR / 4;
		// One quarter in: the quarterly policy charges, the annual one only accrues.
		assert!(assess(&quarterly, &snapshot(), nav("1.5"), Trigger::Period, quarter).unwrap().crystallized);
		assert!(!assess(&annual, &snapshot(), nav("1.5"), Trigger::Period, quarter).unwrap().crystallized);
	}

	#[test]
	fn a_zero_policy_charges_nothing_at_all() {
		let none = FeePolicy::new(0, 0, 0, ManagementBasis::InvestedCapital, CrystallizationPeriod::Annual).unwrap();
		assert!(none.is_zero());
		let charge = assess(&none, &snapshot(), nav("2"), Trigger::Period, YEAR * 3).unwrap();
		assert_eq!(charge.due, Usdt::ZERO);
		assert!(charge.is_empty());
	}

	#[test]
	fn rates_above_the_ceilings_are_refused() {
		// The issue this closes: one administrator set 10000 bps and the sweeper took the
		// holding. The constructor is the first wall, the schema CHECK the second.
		assert!(FeePolicy::new(MAX_MANAGEMENT_BPS + 1, 0, 0, ManagementBasis::InvestedCapital, CrystallizationPeriod::Annual).is_err());
		assert!(FeePolicy::new(10_000, 0, 0, ManagementBasis::InvestedCapital, CrystallizationPeriod::Annual).is_err());
		assert!(FeePolicy::new(0, MAX_PERFORMANCE_BPS + 1, 0, ManagementBasis::InvestedCapital, CrystallizationPeriod::Annual).is_err());
		assert!(FeePolicy::new(0, 0, 10_001, ManagementBasis::InvestedCapital, CrystallizationPeriod::Annual).is_err());
		// The ceilings themselves, and a full hurdle, are legal terms.
		assert!(FeePolicy::new(MAX_MANAGEMENT_BPS, MAX_PERFORMANCE_BPS, 10_000, ManagementBasis::MarketValue, CrystallizationPeriod::Monthly).is_ok());
	}

	fn policy(management: u32, performance: u32, hurdle: u32, basis: ManagementBasis, period: CrystallizationPeriod) -> FeePolicy {
		FeePolicy::new(management, performance, hurdle, basis, period).unwrap()
	}

	#[test]
	fn the_house_envelope_is_two_and_twenty_on_invested_capital_annually_with_any_hurdle() {
		use CrystallizationPeriod::*;
		use ManagementBasis::*;
		assert!(FeePolicy::HOUSE.within_house_envelope());
		assert!(FeePolicy::NONE.within_house_envelope());
		assert!(policy(200, 2_000, 800, InvestedCapital, Annual).within_house_envelope(), "the hurdle is free");
		assert!(!policy(201, 2_000, 0, InvestedCapital, Annual).within_house_envelope());
		assert!(!policy(200, 2_001, 0, InvestedCapital, Annual).within_house_envelope());
		assert!(!policy(200, 2_000, 0, MarketValue, Annual).within_house_envelope());
		assert!(!policy(200, 2_000, 0, InvestedCapital, Quarterly).within_house_envelope());
	}

	#[test]
	fn tightening_is_judged_leg_by_leg() {
		use CrystallizationPeriod::*;
		use ManagementBasis::*;
		let house = FeePolicy::HOUSE;
		assert!(!house.tightens_from(&house), "the same terms tighten nothing");
		assert!(policy(201, 2_000, 0, InvestedCapital, Annual).tightens_from(&house), "management up");
		assert!(policy(200, 2_001, 0, InvestedCapital, Annual).tightens_from(&house), "performance up");
		assert!(policy(200, 2_000, 0, MarketValue, Annual).tightens_from(&house), "basis onto the mark");
		assert!(policy(200, 2_000, 0, InvestedCapital, Quarterly).tightens_from(&house), "crystallizing more often");
		let hurdled = policy(200, 2_000, 800, InvestedCapital, Annual);
		assert!(policy(200, 2_000, 700, InvestedCapital, Annual).tightens_from(&hurdled), "hurdle down");
		assert!(!hurdled.tightens_from(&house), "adding a hurdle only helps the investor");
		// Loosening every leg is not a tightening.
		assert!(!policy(100, 1_000, 500, InvestedCapital, Annual).tightens_from(&house));
		// Moving the basis BACK to invested capital is a loosening, whatever else stays.
		let marked = policy(200, 2_000, 0, MarketValue, Annual);
		assert!(!house.tightens_from(&marked));
		// One leg up and another down still tightens: the investor it hurts is not the
		// investor it helps.
		assert!(policy(300, 1_000, 0, InvestedCapital, Annual).tightens_from(&house));
	}

	#[test]
	fn the_owners_are_needed_exactly_for_a_tightening_that_leaves_the_envelope() {
		use CrystallizationPeriod::*;
		use ManagementBasis::*;
		// From nothing to the house terms: tightens, but inside the envelope — an admin's call.
		assert_eq!(requirement_for(None, &FeePolicy::HOUSE), ChangeRequirement::Admin);
		// From nothing straight past the envelope: the owners.
		assert_eq!(requirement_for(None, &policy(300, 2_000, 0, InvestedCapital, Annual)), ChangeRequirement::OwnerConsilium);
		// From the house terms to a dearer schedule: the owners.
		assert_eq!(
			requirement_for(Some(&FeePolicy::HOUSE), &policy(200, 2_500, 0, InvestedCapital, Annual)),
			ChangeRequirement::OwnerConsilium
		);
		assert_eq!(
			requirement_for(Some(&FeePolicy::HOUSE), &policy(200, 2_000, 0, MarketValue, Annual)),
			ChangeRequirement::OwnerConsilium
		);
		assert_eq!(
			requirement_for(Some(&FeePolicy::HOUSE), &policy(200, 2_000, 0, InvestedCapital, Monthly)),
			ChangeRequirement::OwnerConsilium
		);
		// LOOSENING from outside the envelope stays outside it — and is still an admin's call:
		// the requirement is about making things worse for the investor, not about where the
		// terms end up.
		let dear = policy(400, 4_000, 0, MarketValue, Monthly);
		assert_eq!(requirement_for(Some(&dear), &policy(300, 4_000, 0, MarketValue, Monthly)), ChangeRequirement::Admin);
		// A tightening that stays inside the envelope: an admin's call.
		assert_eq!(
			requirement_for(Some(&policy(100, 1_000, 0, InvestedCapital, Annual)), &FeePolicy::HOUSE),
			ChangeRequirement::Admin
		);
		// Adding a hurdle to terms outside the envelope loosens them: an admin's call.
		assert_eq!(requirement_for(Some(&dear), &policy(400, 4_000, 500, MarketValue, Monthly)), ChangeRequirement::Admin);
		// Re-stating the same out-of-envelope terms tightens nothing.
		assert_eq!(requirement_for(Some(&dear), &dear), ChangeRequirement::Admin);
	}

	#[test]
	fn the_notice_period_binds_only_while_someone_holds_units() {
		const NOW: i64 = 1_700_000_000;
		// No holders: the operator's request stands, and "as soon as allowed" is now.
		assert_eq!(earliest_effective_from(NOW, 0, false), NOW);
		assert_eq!(earliest_effective_from(NOW, NOW + 10, false), NOW + 10);
		// Holders: never earlier than the notice period, however early the request.
		assert_eq!(earliest_effective_from(NOW, 0, true), NOW + MIN_NOTICE_SECS);
		assert_eq!(earliest_effective_from(NOW, NOW + 10, true), NOW + MIN_NOTICE_SECS);
		// A request beyond the notice period is honoured as it is.
		assert_eq!(earliest_effective_from(NOW, NOW + 2 * MIN_NOTICE_SECS, true), NOW + 2 * MIN_NOTICE_SECS);
	}

	#[test]
	fn the_subject_encoding_is_domain_separated_and_binds_every_field() {
		let subject = FeePolicySubject {
			change_id: FeePolicyChangeId::from_raw(uuid::Uuid::from_u128(0x233)),
			service: svc(),
			from: FeePolicy::NONE,
			to: FeePolicy::HOUSE,
			requested_effective_from: 0,
		};
		assert!(subject.canonical_bytes().starts_with(FeePolicySubject::DOMAIN));
		assert_eq!(FeePolicySubject::DOMAIN, b"banking.v1.FeePolicySubject\x00");
		assert_eq!(subject.canonical_bytes(), subject.canonical_bytes());
		let other_change = FeePolicySubject {
			change_id: FeePolicyChangeId::from_raw(uuid::Uuid::from_u128(0x234)),
			..subject.clone()
		};
		assert_ne!(
			subject.canonical_bytes(),
			other_change.canonical_bytes(),
			"an approval of one change is not a signature over another"
		);
		let dearer = FeePolicySubject {
			to: FeePolicy::new(300, 2_000, 0, ManagementBasis::InvestedCapital, CrystallizationPeriod::Annual).unwrap(),
			..subject.clone()
		};
		assert_ne!(subject.canonical_bytes(), dearer.canonical_bytes());
		let later = FeePolicySubject {
			requested_effective_from: 1,
			..subject.clone()
		};
		assert_ne!(subject.canonical_bytes(), later.canonical_bytes());
		let swapped = FeePolicySubject {
			from: FeePolicy::HOUSE,
			to: FeePolicy::NONE,
			..subject.clone()
		};
		assert_ne!(subject.canonical_bytes(), swapped.canonical_bytes(), "the direction of the change is part of what is signed");
	}

	#[test]
	fn change_vocabulary_round_trips_through_its_stored_strings() {
		for state in [
			FeePolicyChangeState::AwaitingConsilium,
			FeePolicyChangeState::Scheduled,
			FeePolicyChangeState::Active,
			FeePolicyChangeState::Superseded,
			FeePolicyChangeState::Rejected,
			FeePolicyChangeState::Cancelled,
		] {
			assert_eq!(FeePolicyChangeState::parse(state.as_str()).unwrap(), state);
			assert_eq!(state.is_pending(), matches!(state, FeePolicyChangeState::AwaitingConsilium | FeePolicyChangeState::Scheduled));
		}
		for requirement in [ChangeRequirement::Admin, ChangeRequirement::OwnerConsilium] {
			assert_eq!(ChangeRequirement::parse(requirement.as_str()).unwrap(), requirement);
		}
		assert!(FeePolicyChangeState::parse("pending").is_err());
		assert!(ChangeRequirement::parse("owner").is_err());
	}

	#[test]
	fn an_empty_charge_cannot_become_an_assessment() {
		let mut snap = snapshot();
		snap.units = Shares::ZERO;
		let charge = assess(&FeePolicy::HOUSE, &snap, nav("1"), Trigger::Period, YEAR).unwrap();
		assert!(FeeAssessment::record(FeeAssessmentId::new(), UserId::new(), svc(), nav("1"), Trigger::Period, charge).is_err());
	}

	#[test]
	fn recording_a_charge_emits_the_clawback_once() {
		let charge = assess(&FeePolicy::HOUSE, &snapshot(), nav("1"), Trigger::Period, YEAR).unwrap();
		let mut assessment = FeeAssessment::record(FeeAssessmentId::new(), UserId::new(), svc(), nav("1"), Trigger::Period, charge).unwrap();
		let events = assessment.drain_events();
		assert_eq!(events.len(), 1);
		assert!(matches!(events[0], FeeEvent::Charged { units, .. } if units == shares("20")));
		assert!(assessment.drain_events().is_empty());
	}

	#[test]
	fn a_settlement_prices_the_fee_units_and_emits_once() {
		let mut settlement = FeeSettlement::record(FeeSettlementId::new(), svc(), shares("20"), nav("1.5")).unwrap();
		assert_eq!(settlement.cash(), usdt("30"));
		let events = settlement.drain_events();
		assert_eq!(events.len(), 1);
		assert!(matches!(events[0], FeeEvent::SharesSettled { .. }));
		// Zero units, and units worth nothing, are both refused (TB rejects a zero transfer).
		assert!(FeeSettlement::record(FeeSettlementId::new(), svc(), Shares::ZERO, nav("1.5")).is_err());
		assert!(FeeSettlement::record(FeeSettlementId::new(), svc(), Shares::from_base_units(1), Nav::from_base_units(1)).is_err());
	}

	#[test]
	fn events_round_trip_through_json() {
		let charge = assess(&FeePolicy::HOUSE, &snapshot(), nav("1"), Trigger::Period, YEAR).unwrap();
		let mut assessment = FeeAssessment::record(FeeAssessmentId::new(), UserId::new(), svc(), nav("1"), Trigger::Period, charge).unwrap();
		let event = assessment.drain_events().pop().unwrap();
		let back: FeeEvent = serde_json::from_str(&serde_json::to_string(&event).unwrap()).unwrap();
		assert!(matches!(back, FeeEvent::Charged { .. }));
	}

	#[test]
	fn policy_vocabulary_round_trips_through_its_stored_strings() {
		for basis in [ManagementBasis::InvestedCapital, ManagementBasis::MarketValue] {
			assert_eq!(ManagementBasis::parse(basis.as_str()).unwrap(), basis);
		}
		for period in [
			CrystallizationPeriod::Monthly,
			CrystallizationPeriod::Quarterly,
			CrystallizationPeriod::SemiAnnual,
			CrystallizationPeriod::Annual,
		] {
			assert_eq!(CrystallizationPeriod::parse(period.as_str()).unwrap(), period);
		}
		assert!(ManagementBasis::parse("aum").is_err());
		assert!(CrystallizationPeriod::parse("daily").is_err());
	}
}
