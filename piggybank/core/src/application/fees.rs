//! Fee use cases — assessing "2 and 20" against a holding, and turning the units it
//! collects into cash.
//!
//! Every write here is the same shape: read the policy, read the holding's *live* unit
//! balance from TigerBeetle, ask [`domain::fees::assess`] what is owed, and persist only
//! if something is actually collectable. Nothing is charged when the fund has no policy,
//! when the policy is all zeros, when the investor has no position, or when the charge
//! floors to nothing — and in the last case the clocks are deliberately left alone so
//! the accrual simply continues into the next sweep.
//!
//! **The NAV a fee is charged at is the dealing NAV**, staleness guard included. That is
//! a safety property, not an accident: if an operator stops posting marks, fees stop
//! accruing rather than accruing against a price nobody has confirmed. A fee is the one
//! charge the operator sets, prices, and collects, so it gets the same guard as an
//! investor's own dealing.

use domain::{
	balance::{LedgerAccountKey, ServiceId},
	error::DomainError,
	fees::{self, FeeAssessment, FeeAssessmentId, FeeCharge, FeePolicy, FeeSettlement, FeeSettlementId, PositionSnapshot, Trigger},
	money::{Nav, Shares, Usdt},
	users::UserId,
};
use tokio::sync::Notify;

use crate::{
	application::{allocations as allocations_app, funds::dealing_nav},
	ports::{
		allocations::AllocationRegistry,
		fees::{AssessmentRecord, FeeAssessments, FeePolicies, FeeSettlements, PositionAccruals, SettlementRecord},
		ledger::Ledger,
		nav::NavMarks,
	},
};

/// A holding's fee position for display: what has accrued since the last charge, and
/// what is still owed from earlier ones. `total` is what would be taken if the fee were
/// assessed right now, which is what a "value net of fees" figure must subtract.
pub struct AccruedFees {
	pub management: Usdt,
	pub performance: Usdt,
	pub debt: Usdt,
	pub total: Usdt,
	/// The investor's own high-water mark — the price their performance fee is measured
	/// from. Worth showing: it is the single number that explains why two investors in
	/// the same fund owe different fees.
	pub high_water_mark: Nav,
}

/// Assess one holding and charge it if anything is collectable.
///
/// Returns `None` — not an error — for every "nothing to do" case, because the sweeper
/// walks thousands of positions and a fund without a policy is normal, not exceptional.
#[allow(clippy::too_many_arguments)]
pub async fn assess_position(
	policies: &dyn FeePolicies,
	accruals: &dyn PositionAccruals,
	assessments: &dyn FeeAssessments,
	ledger: &dyn Ledger,
	nav: &dyn NavMarks,
	relay: &Notify,
	user: UserId,
	service: ServiceId,
	trigger: Trigger,
	now_unix: i64,
) -> Result<Option<FeeAssessment>, DomainError> {
	let Some((policy, snapshot, price)) = load_assessment_inputs(policies, accruals, ledger, nav, user, &service, now_unix).await? else {
		return Ok(None);
	};
	let charge = fees::assess(&policy, &snapshot, price, trigger, now_unix)?;
	if charge.is_empty() {
		return Ok(None);
	}
	let mut assessment = FeeAssessment::record(FeeAssessmentId::new(), user, service, price, trigger, charge)?;
	assessments.charge(&mut assessment).await?;
	relay.notify_one();
	Ok(Some(assessment))
}

/// What a holding owes right now, without charging it — the disclosure behind a
/// position's net-of-fees value. A fund with no policy owes nothing.
pub async fn accrued_fees(
	policies: &dyn FeePolicies,
	accruals: &dyn PositionAccruals,
	ledger: &dyn Ledger,
	nav: &dyn NavMarks,
	user: UserId,
	service: &ServiceId,
	now_unix: i64,
) -> Result<Option<AccruedFees>, DomainError> {
	let Some((policy, snapshot, price)) = load_assessment_inputs(policies, accruals, ledger, nav, user, service, now_unix).await? else {
		return Ok(None);
	};
	let charge = fees::assess(&policy, &snapshot, price, Trigger::Period, now_unix)?;
	Ok(Some(disclose(&charge, snapshot.high_water_mark)))
}

/// The accrued view of a charge: the *accrued* performance figure, never the
/// crystallized one, so a position reads the same on both sides of a period boundary.
fn disclose(charge: &FeeCharge, high_water_mark: Nav) -> AccruedFees {
	let total = charge
		.management
		.checked_add(charge.performance_accrued)
		.and_then(|sum| sum.checked_add(charge.debt_opening))
		.unwrap_or(Usdt::ZERO);
	AccruedFees {
		management: charge.management,
		performance: charge.performance_accrued,
		debt: charge.debt_opening,
		total,
		high_water_mark,
	}
}

/// The three reads every assessment needs. `None` whenever there is nothing to assess:
/// no policy, a policy that charges nothing, or no position.
///
/// `units` is the **available** balance from TigerBeetle, not the projection's copy: the
/// projection lags the relay, and a cap computed from a stale figure could try to claw
/// back units the holder no longer has (which TigerBeetle's non-negative flag would then
/// park). Available, not posted, so units already reserved by a queued redemption are
/// left alone and the charge defers into debt instead of racing the burn.
async fn load_assessment_inputs(
	policies: &dyn FeePolicies,
	accruals: &dyn PositionAccruals,
	ledger: &dyn Ledger,
	nav: &dyn NavMarks,
	user: UserId,
	service: &ServiceId,
	now_unix: i64,
) -> Result<Option<(FeePolicy, PositionSnapshot, Nav)>, DomainError> {
	let Some(policy) = policies.find(service).await? else {
		return Ok(None);
	};
	if policy.is_zero() {
		return Ok(None);
	}
	let Some(accrual) = accruals.find(user, service).await? else {
		return Ok(None);
	};
	let price = dealing_nav(nav, service, now_unix).await?;
	let units = Shares::from_base_units(ledger.balance(&LedgerAccountKey::UserShares(service.clone(), user)).await?.available());
	let snapshot = PositionSnapshot {
		units,
		cost_basis: accrual.cost_basis,
		high_water_mark: accrual.high_water_mark,
		debt: accrual.debt,
		accrued_at_unix: accrual.accrued_at_unix,
		crystallized_at_unix: accrual.crystallized_at_unix,
	};
	Ok(Some((policy, snapshot, price)))
}

/// Install or replace a fund's fee terms (operator). The allocation must exist — terms
/// for an unregistered product are always a typo, and the registry is the same gate
/// subscribe runs through.
pub async fn set_policy(policies: &dyn FeePolicies, allocations: &dyn AllocationRegistry, service: ServiceId, policy: FeePolicy, updated_by: &str) -> Result<FeePolicy, DomainError> {
	allocations_app::get(allocations, &service).await?;
	policies.set(&service, policy, updated_by).await?;
	Ok(policy)
}

/// One fund's terms, or `None` when the product charges nothing.
pub async fn get_policy(policies: &dyn FeePolicies, service: &ServiceId) -> Result<Option<FeePolicy>, DomainError> {
	policies.find(service).await
}

/// Every configured policy.
pub async fn list_policies(policies: &dyn FeePolicies) -> Result<Vec<(ServiceId, FeePolicy)>, DomainError> {
	policies.list().await
}

/// One investor's fee statement, newest first.
pub async fn list_assessments(assessments: &dyn FeeAssessments, user: UserId) -> Result<Vec<AssessmentRecord>, DomainError> {
	assessments.list_by_user(user).await
}

/// Every charge against one fund (operator).
pub async fn list_fund_assessments(assessments: &dyn FeeAssessments, service: &ServiceId) -> Result<Vec<AssessmentRecord>, DomainError> {
	assessments.list_by_service(service).await
}

/// The manager's uncollected fee units in a fund, and what they are worth right now.
pub async fn fee_shares(ledger: &dyn Ledger, nav: &dyn NavMarks, service: &ServiceId, now_unix: i64) -> Result<(Shares, Usdt), DomainError> {
	let units = Shares::from_base_units(ledger.balance(&LedgerAccountKey::FeeShares(service.clone())).await?.available());
	let price = dealing_nav(nav, service, now_unix).await?;
	Ok((units, price.value(units)?))
}

/// Convert accumulated fee units into fee revenue (operator). `units` defaults to the
/// whole accumulated balance.
///
/// This is the **only** operation in the fee plane that moves cash, and it runs once per
/// period for a whole fund rather than once per investor — which is the entire point of
/// collecting in units. It is Read-First gated on the fund's claim covering the payout
/// and **refuses** when short rather than queueing: unlike an investor's redemption,
/// nobody is waiting on this, and a fee that cannot be paid today is simply left
/// accumulating as units at no cost.
#[allow(clippy::too_many_arguments)]
pub async fn settle_fee_shares(
	settlements: &dyn FeeSettlements,
	ledger: &dyn Ledger,
	nav: &dyn NavMarks,
	relay: &Notify,
	service: ServiceId,
	units: Option<Shares>,
	settled_by: &str,
	now_unix: i64,
) -> Result<FeeSettlement, DomainError> {
	let held = Shares::from_base_units(ledger.balance(&LedgerAccountKey::FeeShares(service.clone())).await?.available());
	let units = units.unwrap_or(held);
	if units.is_zero() {
		return Err(DomainError::Validation("no fee units to settle".into()));
	}
	if units > held {
		return Err(DomainError::Validation("cannot settle more fee units than the fund has accumulated".into()));
	}
	let price = dealing_nav(nav, &service, now_unix).await?;
	let mut settlement = FeeSettlement::record(FeeSettlementId::new(), service.clone(), units, price)?;
	let fund = ledger.balance(&LedgerAccountKey::ServiceClaim(service)).await?;
	if Usdt::from_base_units(fund.available()) < settlement.cash() {
		return Err(DomainError::Validation(
			"the fund's claim cannot cover this fee settlement — top it up or settle fewer units".into(),
		));
	}
	settlements.settle(&mut settlement, settled_by).await?;
	relay.notify_one();
	Ok(settlement)
}

/// Settlement history for one fund.
pub async fn list_settlements(settlements: &dyn FeeSettlements, service: &ServiceId) -> Result<Vec<SettlementRecord>, DomainError> {
	settlements.list_by_service(service).await
}
