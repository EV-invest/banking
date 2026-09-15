//! Fee use cases — assessing "2 and 20" against a holding, and turning the units it
//! collects into cash.
//!
//! Every write here is the same shape: read the policy, read the holding's *live* unit
//! balances from TigerBeetle, ask [`domain::fees::assess`] what is owed, and persist only
//! if something is actually owed. Nothing is charged when the fund has no policy, when
//! the policy is all zeros, when the investor has no position, or when the charge floors
//! to nothing — and in the last case the clocks are deliberately left alone so the
//! accrual simply continues into the next sweep. A charge that is owed but cannot be
//! collected right now (the units are escrowed by a resting sell or reserved by a queued
//! redemption) is NOT one of those cases: it is recorded, carried whole as debt, and the
//! clock moves — see [`domain::fees::FeeCharge::is_empty`].
//!
//! **The NAV a fee is charged at is the dealing NAV**, staleness guard included. That is
//! a safety property, not an accident: if an operator stops posting marks, fees stop
//! accruing rather than accruing against a price nobody has confirmed. A fee is the one
//! charge the operator sets, prices, and collects, so it gets the same guard as an
//! investor's own dealing.

use std::collections::{HashMap, HashSet};

use domain::{
	balance::{LedgerAccountKey, ServiceId},
	consilium::{Consilium, ConsiliumId, ConsiliumTerms},
	error::DomainError,
	fees::{self, ChangeRequirement, FeeAssessment, FeeAssessmentId, FeeCharge, FeePolicy, FeePolicyChangeId, FeePolicySubject, FeeSettlement, FeeSettlementId, PositionSnapshot, Trigger},
	money::{Nav, Shares, Usdt},
	users::UserId,
};
use tokio::sync::Notify;

use crate::{
	application::{
		allocations as allocations_app,
		consilium::{mint_credential, require_governance_mail, require_settled_roster},
		funds::dealing_nav,
	},
	infrastructure::consilium::digest,
	ports::{
		allocations::AllocationRegistry,
		consilium::ConsiliumRepository,
		fees::{
			AssessmentRecord, ConsiliumOpening, FeeAssessments, FeePolicies, FeePolicyChange, FeePolicyChanges, FeeSettlements, NewFeePolicyChange, PolicyRecord, PositionAccruals,
			SettlementRecord,
		},
		ledger::Ledger,
		nav::NavMarks,
		redemptions::RedemptionRepository,
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

/// Assess one holding and charge it if anything is owed — collecting what the free
/// holding can give up and carrying the rest as debt.
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
	// Conditional on the accrual clock still being the one we assessed against. Losing
	// that race is a non-event like every other "nothing to do" here: the winner charged
	// this window, and re-charging it would bill the investor twice for the same days.
	if !assessments.charge(&mut assessment, snapshot.accrued_at_unix, now_unix).await? {
		return Ok(None);
	}
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
/// Both unit figures come from TigerBeetle, not the projection's copy: the projection
/// lags the relay, and a cap computed from a stale figure could try to claw back units
/// the holder no longer has (which TigerBeetle's non-negative flag would then park).
///
/// The fee is owed on the whole position — the holding's **posted** balance plus what a
/// resting sell has moved into the book's escrow. Posted, not available, because units a
/// queued redemption has reserved are still the holder's until the burn lands. The
/// clawback, by contrast, may only take the holding's **available** balance: the escrow
/// belongs to the order until the book releases it, and the reserve to the redemption,
/// so the charge defers into debt instead of racing either.
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
	let holding = ledger.balance(&LedgerAccountKey::UserShares(service.clone(), user)).await?;
	let escrowed = ledger.balance(&LedgerAccountKey::BookShares(service.clone(), user)).await?.posted;
	let units = Shares::from_base_units(holding.posted)
		.checked_add(Shares::from_base_units(escrowed))
		.ok_or_else(|| DomainError::Validation("position units overflow".into()))?;
	let snapshot = PositionSnapshot {
		units,
		collectable: Shares::from_base_units(holding.available()),
		cost_basis: accrual.cost_basis,
		high_water_mark: accrual.high_water_mark,
		debt: accrual.debt,
		accrued_at_unix: accrual.accrued_at_unix,
		crystallized_at_unix: accrual.crystallized_at_unix,
	};
	Ok(Some((policy, snapshot, price)))
}

/// The driven ports a change of terms borrows. Only [`schedule_policy`] needs the consilium
/// side; reading and cancelling touch the fee plane alone.
pub struct FeePolicyPorts<'a> {
	pub policies: &'a dyn FeePolicies,
	pub changes: &'a dyn FeePolicyChanges,
	pub allocations: &'a dyn AllocationRegistry,
	pub consilia: &'a dyn ConsiliumRepository,
	/// Base URL the owners' emailed approval link is built on.
	pub approval_url_base: &'a str,
	/// Whether a governance mailer is wired — see
	/// [`require_governance_mail`](crate::application::consilium::require_governance_mail).
	pub governance_mail_wired: bool,
}

/// A change of terms as the operator asks for it.
pub struct PolicyChangeRequest {
	pub service: ServiceId,
	pub policy: FeePolicy,
	/// `0` = as soon as the notice period allows.
	pub requested_effective_from_unix: i64,
	/// Why, in the requester's words. Required when the owners must approve.
	pub reason: String,
}

/// Propose a change of a fund's fee terms (operator).
///
/// The requirement is decided by the pure rule in [`fees::requirement_for`]: the owners'
/// consilium exactly when the change tightens the terms beyond the house envelope, one
/// `AllocationManage` holder otherwise. An administrator's change is scheduled at once and
/// the holders are notified; a change needing the owners opens a consilium the requester
/// must be an owner to open, and the holders are notified when the owners carry it. Either
/// way the terms bind no earlier than the notice period while anyone holds units — see
/// `docs/FEES.md` § "Changing the terms".
pub async fn schedule_policy(ports: &FeePolicyPorts<'_>, requester: UserId, request: PolicyChangeRequest, now: i64) -> Result<FeePolicyChange, DomainError> {
	// The registry is the same gate subscribe runs through: terms for an unregistered
	// product are always a typo.
	allocations_app::get(ports.allocations, &request.service).await?;
	if request.requested_effective_from_unix > now.saturating_add(fees::MAX_EFFECTIVE_FROM_HORIZON_SECS) {
		return Err(DomainError::Validation("effective_from may be at most 366 days ahead".into()));
	}
	let current = ports.policies.find(&request.service).await?;
	let requirement = fees::requirement_for(current.as_ref(), &request.policy);
	fees::validate_reason(&request.reason, requirement == ChangeRequirement::OwnerConsilium)?;
	// The notice IS the protection, on either path. A change over a held product with no
	// mailer would be scheduled, its notices queued into a relay that never runs, and bind
	// 24h later on holders who were told nothing — the guardrail looking healthy the whole
	// time. A product nobody holds owes nobody a notice and may proceed.
	if !ports.governance_mail_wired && ports.changes.holder_count(&request.service).await? > 0 {
		return Err(DomainError::Conflict(
			"governance mail is not configured, so the holders of this product could not be given notice of a change of terms. Build with the `concierge_governance_mail` feature and configure the concierge mail relay first.".into(),
		));
	}
	let change = NewFeePolicyChange {
		id: FeePolicyChangeId::new(),
		service: request.service.clone(),
		policy: request.policy,
		requirement,
		requested_effective_from_unix: request.requested_effective_from_unix,
		requested_by: requester.to_string(),
		reason: request.reason.clone(),
		now_unix: now,
	};
	match requirement {
		ChangeRequirement::Admin => ports.changes.schedule(&change, None).await,
		ChangeRequirement::OwnerConsilium => {
			require_governance_mail(ports.governance_mail_wired)?;
			require_settled_roster(ports.consilia, now).await?;
			let owners = ports.consilia.owner_roster().await?;
			// Said in the fee plane's words before the aggregate says it in its own: an
			// administrator who is not an owner needs to know WHY this particular change is
			// out of their reach, not merely that a consilium is.
			if !owners.contains(&requester) {
				return Err(DomainError::Forbidden(
					"this change tightens the terms beyond the house envelope, so it needs the owners' consilium — only a fund owner may propose it".into(),
				));
			}
			let terms = ConsiliumTerms::FeePolicy(FeePolicySubject {
				change_id: change.id,
				service: request.service,
				from: current,
				to: request.policy,
				reason: request.reason,
				requested_effective_from: request.requested_effective_from_unix,
			});
			let payload_hash = digest(&terms.canonical_bytes());
			let mut consilium = Consilium::open(ConsiliumId::new(), terms, payload_hash, requester, &owners, now)?;
			let credentials = consilium.eligible().iter().map(|voter| mint_credential(*voter)).collect::<Result<Vec<_>, _>>()?;
			ports
				.changes
				.schedule(
					&change,
					Some(ConsiliumOpening {
						consilium: &mut consilium,
						credentials: &credentials,
						approval_url_base: ports.approval_url_base,
					}),
				)
				.await
		}
	}
}

/// Withdraw a pending change (operator). One still awaiting the owners takes its consilium
/// down with it — and for that reason only the owner who proposed it, or another owner,
/// may withdraw it: an administrator who could not have opened the quorum must not be able
/// to close it either, or every consilium-gated change is one `AllocationManage` holder
/// away from never reaching a vote.
pub async fn cancel_change(
	changes: &dyn FeePolicyChanges,
	consilia: &dyn ConsiliumRepository,
	service: &ServiceId,
	id: FeePolicyChangeId,
	by: UserId,
	now: i64,
) -> Result<FeePolicyChange, DomainError> {
	let change = changes.find(id).await?.filter(|change| &change.service == service).ok_or_else(|| DomainError::NotFound {
		entity: "fee policy change",
		id: id.to_string(),
	})?;
	if change.requirement == ChangeRequirement::OwnerConsilium && change.requested_by != by.to_string() && !consilia.owner_roster().await?.contains(&by) {
		return Err(DomainError::Forbidden(
			"a change awaiting the owners' consilium may be withdrawn only by the owner who proposed it or by another owner".into(),
		));
	}
	changes.cancel(service, id, &by.to_string(), now).await
}

/// Who is reading a product's terms. A product hidden from `caller` answers as unregistered
/// — `NotFound`, exactly as `GetAllocation` and `GetFundNav` answer — unless `unrestricted`
/// (an `AllocationManage` holder, decided at the boundary): the terms are part of what the
/// product IS, and a product a caller cannot see has no terms to show them either.
pub struct PolicyReader<'a> {
	pub allocations: &'a dyn AllocationRegistry,
	pub caller: UserId,
	pub unrestricted: bool,
}

impl PolicyReader<'_> {
	async fn require_visible(&self, service: &ServiceId) -> Result<(), DomainError> {
		allocations_app::get_for(self.allocations, service, self.caller, self.unrestricted).await.map(|_| ())
	}

	/// The products whose terms this reader may list: every one for an unrestricted
	/// reader, else the catalog as they see it — the same set `ListAllocations` shows them.
	async fn visible_set(&self) -> Result<Option<HashSet<ServiceId>>, DomainError> {
		if self.unrestricted {
			return Ok(None);
		}
		let catalog = allocations_app::list_for(self.allocations, self.caller, false).await?;
		Ok(Some(catalog.into_iter().map(|record| record.allocation.service().clone()).collect()))
	}
}

/// A product's whole history of terms, newest version first.
pub async fn list_changes(changes: &dyn FeePolicyChanges, reader: &PolicyReader<'_>, service: &ServiceId) -> Result<Vec<FeePolicyChange>, DomainError> {
	reader.require_visible(service).await?;
	changes.list(service).await
}

/// The live terms of a product together with the change on its way, if any — what every
/// reader of a policy is shown, holder or not: the terms they are on, and the terms coming.
pub struct PolicyView {
	pub current: Option<PolicyRecord>,
	pub pending: Option<FeePolicyChange>,
}

pub async fn policy_view(policies: &dyn FeePolicies, changes: &dyn FeePolicyChanges, reader: &PolicyReader<'_>, service: &ServiceId) -> Result<PolicyView, DomainError> {
	reader.require_visible(service).await?;
	Ok(PolicyView {
		current: policies.current(service).await?,
		pending: changes.pending(service).await?,
	})
}

/// One fund's terms, or `None` when the product charges nothing.
pub async fn get_policy(policies: &dyn FeePolicies, service: &ServiceId) -> Result<Option<FeePolicy>, DomainError> {
	policies.find(service).await
}

/// Every configured policy the reader may see, each with its pending change.
pub async fn list_policies(policies: &dyn FeePolicies, changes: &dyn FeePolicyChanges, reader: &PolicyReader<'_>) -> Result<Vec<(ServiceId, PolicyView)>, DomainError> {
	let visible = reader.visible_set().await?;
	let mut views = Vec::new();
	for (service, current) in policies.list().await? {
		if visible.as_ref().is_some_and(|visible| !visible.contains(&service)) {
			continue;
		}
		let pending = changes.pending(&service).await?;
		views.push((service, PolicyView { current: Some(current), pending }));
	}
	Ok(views)
}

/// How many consecutive failed promotions of one change turn the per-tick warning into an
/// error: ten minutes of the same change refusing to land is no longer a transient.
pub const PROMOTION_FAILURES_BEFORE_ERROR: u32 = 10;

/// Promote every scheduled change whose moment has come. Driven by the fee sweeper; returns
/// how many were promoted. A failure on one product warns and moves on — the change stays
/// `scheduled` and the next tick retries it — so one product's broken position cannot hold
/// every other product's terms hostage. `failures` is the sweeper's memory of consecutive
/// failures per change: past [`PROMOTION_FAILURES_BEFORE_ERROR`] the line is an error, with
/// the database's own words, because a change that will never land is an operator's problem
/// and a warning per minute is how such a problem hides in a log.
pub async fn promote_due(changes: &dyn FeePolicyChanges, now: i64, failures: &mut HashMap<FeePolicyChangeId, u32>) -> Result<usize, DomainError> {
	let mut promoted = 0usize;
	for id in changes.due(now).await? {
		match changes.promote(id, now).await {
			Ok(true) => {
				promoted += 1;
				failures.remove(&id);
			}
			Ok(false) => {
				failures.remove(&id);
			}
			Err(err) => {
				let streak = failures.entry(id).or_insert(0);
				*streak = streak.saturating_add(1);
				if *streak >= PROMOTION_FAILURES_BEFORE_ERROR {
					tracing::error!(change_id = %id, consecutive_failures = *streak, "fee policy: a scheduled change keeps failing to promote: {err}");
				} else {
					tracing::warn!(change_id = %id, consecutive_failures = *streak, "fee policy: could not promote a scheduled change (will retry): {err}");
				}
			}
		}
	}
	Ok(promoted)
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
///
/// # The queue is reserved before the manager is paid
///
/// Queuing a redemption reserves the investor's *units* (a pending burn) but **no cash** —
/// there is none to reserve, because the redemption is priced at settle. So the fund's
/// claim carries no trace of what the queue will shortly cost, and a settlement gated only
/// on `available` would happily hand the manager money the queue is about to need, leaving
/// investors unpayable while the fee revenue sits collected.
///
/// This is the ordinary fund-accounting holdback: accrued-but-unpaid fees are a liability
/// of the fund, and the manager draws only what is left once the fund's obligations are
/// covered. So the queue is priced at the same dealing NAV and reserved first. It is an
/// estimate — the queue settles at whatever mark is posted then, not this one — but it is
/// the only honest estimate available, and erring toward the investor is the direction
/// every rounding decision in this plane already leans.
#[allow(clippy::too_many_arguments)]
pub async fn settle_fee_shares(
	settlements: &dyn FeeSettlements,
	ledger: &dyn Ledger,
	nav: &dyn NavMarks,
	redemptions: &dyn RedemptionRepository,
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
	let fund = ledger.balance(&LedgerAccountKey::ServiceClaim(service.clone())).await?;
	let reserved = queued_redemption_cash(redemptions, &service, price).await?;
	let required = settlement
		.cash()
		.checked_add(reserved)
		.ok_or_else(|| DomainError::Repository("fee settlement plus the queue overflows".into()))?;
	if Usdt::from_base_units(fund.available()) < required {
		return Err(DomainError::Validation(if reserved.is_zero() {
			"the fund's claim cannot cover this fee settlement — top it up or settle fewer units".into()
		} else {
			format!(
				"the fund's claim cannot cover this fee settlement on top of {reserved} USDT of queued redemptions — \
				 settle the queue first, top the fund up, or settle fewer units"
			)
		}));
	}
	settlements.settle(&mut settlement, settled_by).await?;
	relay.notify_one();
	Ok(settlement)
}

/// What the queued redemptions for one fund would cost at `price`.
///
/// The queue read is cross-fund (it backs the operator's single "clear the queue" screen),
/// so it is filtered here rather than growing a second query for one caller.
async fn queued_redemption_cash(redemptions: &dyn RedemptionRepository, service: &ServiceId, price: Nav) -> Result<Usdt, DomainError> {
	let mut total = Usdt::ZERO;
	for queued in redemptions.list_queued().await? {
		if &queued.service != service {
			continue;
		}
		let cash = price.value(queued.units)?;
		total = total.checked_add(cash).ok_or_else(|| DomainError::Repository("queued redemption cash overflows".into()))?;
	}
	Ok(total)
}

/// Settlement history for one fund.
pub async fn list_settlements(settlements: &dyn FeeSettlements, service: &ServiceId) -> Result<Vec<SettlementRecord>, DomainError> {
	settlements.list_by_service(service).await
}
