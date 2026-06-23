//! Fund (share) use cases — the service currency.
//!
//! NAV is **derived**, not posted directly: an operator posts a fund's total AUM and the
//! handler reads `units_outstanding` live from TigerBeetle to compute
//! `NAV = AUM / units_outstanding` (frozen until the next mark). Subscribe/redeem
//! (later slices) deal on the latest mark — a deliberate *backward-pricing* tradeoff,
//! guarded by a **staleness** check; the operator post is guarded by a **move** check,
//! because the AUM input is the most dangerous seam in the system ("trusted" ≠ "safe").

use domain::{
	balance::{LedgerAccountKey, ServiceId},
	error::DomainError,
	money::{Nav, Shares, Usdt},
	redemptions::{Redemption, RedemptionId},
	subscriptions::{Subscription, SubscriptionId},
	users::UserId,
};
use tokio::sync::Notify;
use uuid::Uuid;

use crate::ports::{
	FundPositionReader, RedemptionRepository, SubscriptionRepository,
	ledger::Ledger,
	nav::{NavRepository, Valuation},
};

/// A derived NAV that jumps more than this (percent) from the previous mark is rejected
/// unless the operator passes an override — the fat-finger guard on the AUM trust seam.
pub const MAX_NAV_MOVE_PCT: u128 = 50;
/// A mark older than this (seconds) is stale; subscribe/redeem refuse to deal on it
/// rather than price off a drifted NAV (the backward-pricing arbitrage guard). 24h for v1.
pub const MAX_NAV_AGE_SECS: i64 = 24 * 60 * 60;
/// A user's position in one fund, assembled from the live unit balance (TigerBeetle),
/// the current NAV, and the cost-basis projection. `value = units × nav`; P&L is
/// `value − cost_basis` (computed at the wire boundary, where a signed value is natural).
pub struct PositionView {
	pub service: ServiceId,
	pub units: Shares,
	pub nav: Nav,
	pub value: Usdt,
	pub cost_basis: Usdt,
	/// Unix seconds of the NAV mark used (0 when on the bootstrap seed NAV).
	pub nav_as_of: i64,
}

/// A fund's current price and freshness for display.
pub struct FundNavView {
	pub service: ServiceId,
	pub nav: Nav,
	/// The last posted AUM, or `None` when the fund is still on the seed NAV.
	pub aum: Option<Usdt>,
	pub units_outstanding: Shares,
	/// Unix seconds of the latest mark (0 = never marked / seed).
	pub posted_at: i64,
	pub stale: bool,
}

/// The current NAV plus whether it is fresh enough to deal on (`now − posted_at ≤
/// MAX_NAV_AGE_SECS`). A fund with no mark yet uses the seed NAV and is always fresh
/// (nothing to be stale against). Subscribe/redeem call this before pricing.
pub async fn dealing_nav(nav: &dyn NavRepository, service: &ServiceId, now_unix: i64) -> Result<Nav, DomainError> {
	match nav.current(service).await? {
		Some(v) => {
			if now_unix.saturating_sub(v.posted_at_unix) > MAX_NAV_AGE_SECS {
				return Err(DomainError::Validation("fund nav is stale — a fresh valuation is required before dealing".into()));
			}
			Ok(v.nav)
		}
		None => Ok(Nav::SEED),
	}
}

/// A user subscribes `cash` of their free balance into `service`, minting
/// `floor(cash / NAV)` units at the current (fresh) NAV. Read-First confirms the
/// spendable unified claim covers the cash (TigerBeetle's flag is the backstop); the
/// staleness guard refuses to deal on a drifted mark. The relay then posts the cash move
/// (`Dr UserClaim / Cr ServiceClaim`) and the unit mint (`Dr UserShares / Cr
/// SharesOutstanding`) — cash-leg first, so an insufficient claim parks before any mint.
#[allow(clippy::too_many_arguments)]
pub async fn subscribe(
	subscriptions: &dyn SubscriptionRepository,
	ledger: &dyn Ledger,
	nav: &dyn NavRepository,
	relay: &Notify,
	user: UserId,
	service: ServiceId,
	cash: Usdt,
	now_unix: i64,
) -> Result<Subscription, DomainError> {
	let claim = ledger.balance(&LedgerAccountKey::UserClaim(user)).await?;
	if Usdt::from_base_units(claim.available()) < cash {
		return Err(DomainError::Validation("insufficient available balance to subscribe".into()));
	}
	let price = dealing_nav(nav, &service, now_unix).await?;
	let mut subscription = Subscription::open(SubscriptionId::new(), user, service, cash, price)?;
	subscriptions.open(&mut subscription).await?;
	relay.notify_one();
	Ok(subscription)
}

/// A user redeems `units` of `service` back to cash. Read-First confirms the user holds
/// the units (TigerBeetle's flag is the over-redeem backstop); the staleness guard
/// refuses to deal on a drifted mark. The redemption is **accepted and queued**: the
/// relay reserves a pending burn now, and the cash is priced + paid at **settle**. If the
/// fund's claim can already cover the payout, this settles immediately via a **separate**
/// command (never co-emitting `Requested`+`Settled`, which would race the burn reserve);
/// otherwise it stays `Queued` for an operator `settle_redemption` once the fund tops up.
#[allow(clippy::too_many_arguments)]
pub async fn request_redemption(
	redemptions: &dyn RedemptionRepository,
	ledger: &dyn Ledger,
	nav: &dyn NavRepository,
	relay: &Notify,
	user: UserId,
	service: ServiceId,
	units: Shares,
	now_unix: i64,
) -> Result<Redemption, DomainError> {
	let holding = ledger.balance(&LedgerAccountKey::UserShares(service.clone(), user)).await?;
	if Shares::from_base_units(holding.available()) < units {
		return Err(DomainError::Validation("insufficient units to redeem".into()));
	}
	// Fresh NAV (staleness guard) — also the auto-settle liquidity estimate.
	let price = dealing_nav(nav, &service, now_unix).await?;
	let cash_out = price.value(units)?;
	let mut redemption = Redemption::request(RedemptionId::new(), user, service.clone(), units)?;
	redemptions.open(&mut redemption).await?;
	relay.notify_one();
	// Accept-and-queue: settle now (as a separate command) iff the fund's claim can cover
	// the payout; else leave it queued for the treasury worker.
	let fund = ledger.balance(&LedgerAccountKey::ServiceClaim(service)).await?;
	if Usdt::from_base_units(fund.available()) >= cash_out {
		return settle_redemption(redemptions, ledger, nav, relay, redemption.id(), now_unix).await;
	}
	Ok(redemption)
}

/// Settle a queued redemption (the auto follow-on, or an operator once the fund is
/// liquid): prices the cash at the **settle-time** NAV (`units × NAV`) and pays it. The
/// relay posts the burn then the payout, guarded by a Read-First check on the fund claim.
pub async fn settle_redemption(
	redemptions: &dyn RedemptionRepository,
	ledger: &dyn Ledger,
	nav: &dyn NavRepository,
	relay: &Notify,
	id: RedemptionId,
	now_unix: i64,
) -> Result<Redemption, DomainError> {
	let existing = redemptions.find_by_id(id).await?.ok_or_else(|| DomainError::NotFound {
		entity: "redemption",
		id: id.to_string(),
	})?;
	let price = dealing_nav(nav, existing.service(), now_unix).await?;
	// The holding before the burn posts — for the proportional cost-basis reduction.
	let holding = ledger.balance(&LedgerAccountKey::UserShares(existing.service().clone(), existing.user())).await?;
	let units_held = Shares::from_base_units(holding.posted);
	let redemption = redemptions.settle(id, price, units_held).await?;
	relay.notify_one();
	Ok(redemption)
}

/// Cancel a queued redemption (the calling user): the relay voids the burn, returning the
/// units. Ownership is checked here; the aggregate refuses to cancel once settled.
pub async fn cancel_redemption(redemptions: &dyn RedemptionRepository, relay: &Notify, id: RedemptionId, user: UserId) -> Result<Redemption, DomainError> {
	let existing = redemptions.find_by_id(id).await?.ok_or_else(|| DomainError::NotFound {
		entity: "redemption",
		id: id.to_string(),
	})?;
	if existing.user() != user {
		return Err(DomainError::Forbidden("not your redemption".into()));
	}
	let redemption = redemptions.cancel(id).await?;
	relay.notify_one();
	Ok(redemption)
}

/// Fail a queued redemption (operator): the relay voids the burn, returning the units.
pub async fn fail_redemption(redemptions: &dyn RedemptionRepository, relay: &Notify, id: RedemptionId) -> Result<Redemption, DomainError> {
	let redemption = redemptions.fail(id).await?;
	relay.notify_one();
	Ok(redemption)
}

/// A user's redemptions (projection), newest first.
pub async fn list_redemptions(redemptions: &dyn RedemptionRepository, user: UserId) -> Result<Vec<Redemption>, DomainError> {
	redemptions.list_by_user(user).await
}

/// The caller's position in one fund: live units × current NAV, with the cost basis for
/// P&L. A fund never subscribed to reports zero units at the seed NAV.
pub async fn get_position(positions: &dyn FundPositionReader, ledger: &dyn Ledger, nav: &dyn NavRepository, user: UserId, service: ServiceId) -> Result<PositionView, DomainError> {
	let cost_basis = positions.find(user, &service).await?.map(|p| p.cost_basis).unwrap_or(Usdt::ZERO);
	build_position_view(ledger, nav, user, service, cost_basis).await
}

/// All of the caller's fund positions with a non-zero unit balance.
pub async fn list_positions(positions: &dyn FundPositionReader, ledger: &dyn Ledger, nav: &dyn NavRepository, user: UserId) -> Result<Vec<PositionView>, DomainError> {
	let mut out = Vec::new();
	for position in positions.list(user).await? {
		let view = build_position_view(ledger, nav, user, position.service, position.cost_basis).await?;
		if !view.units.is_zero() {
			out.push(view);
		}
	}
	Ok(out)
}

/// The current NAV + freshness for a fund (the seed NAV when never marked).
pub async fn fund_nav_view(nav: &dyn NavRepository, ledger: &dyn Ledger, service: ServiceId, now_unix: i64) -> Result<FundNavView, DomainError> {
	let units_outstanding = Shares::from_base_units(ledger.balance(&LedgerAccountKey::SharesOutstanding(service.clone())).await?.posted);
	Ok(match nav.current(&service).await? {
		Some(v) => FundNavView {
			service,
			nav: v.nav,
			aum: Some(v.aum),
			units_outstanding,
			posted_at: v.posted_at_unix,
			stale: now_unix.saturating_sub(v.posted_at_unix) > MAX_NAV_AGE_SECS,
		},
		None => FundNavView {
			service,
			nav: Nav::SEED,
			aum: None,
			units_outstanding,
			posted_at: 0,
			stale: false,
		},
	})
}
/// Operator posts a fund's total AUM; NAV is derived (`AUM / units_outstanding`, read
/// live from TigerBeetle). Rejects zero units (NAV undefined) and — unless `force` — a
/// move beyond [`MAX_NAV_MOVE_PCT`] vs the last mark. Records the mark (with `posted_by`)
/// and returns it.
pub async fn post_fund_valuation(nav: &dyn NavRepository, ledger: &dyn Ledger, service: ServiceId, aum: Usdt, posted_by: &str, force: bool) -> Result<Valuation, DomainError> {
	let units = Shares::from_base_units(ledger.balance(&LedgerAccountKey::SharesOutstanding(service.clone())).await?.posted);
	// `from_aum` rejects zero units — NAV is undefined with nothing outstanding.
	let derived = Nav::from_aum(aum, units)?;
	if let Some(prev) = nav.current(&service).await?
		&& !force
		&& nav_move_exceeds(prev.nav, derived, MAX_NAV_MOVE_PCT)
	{
		return Err(DomainError::Validation(format!(
			"nav move {} → {derived} exceeds {MAX_NAV_MOVE_PCT}% — pass override to confirm",
			prev.nav
		)));
	}
	let id = Uuid::new_v4();
	let posted_at_unix = nav.record(id, &service, aum, units, derived, posted_by).await?;
	Ok(Valuation {
		service,
		aum,
		units_outstanding: units,
		nav: derived,
		posted_by: posted_by.to_owned(),
		posted_at_unix,
	})
}
/// Assemble a position view: read the live unit balance and the current NAV, value it.
async fn build_position_view(ledger: &dyn Ledger, nav: &dyn NavRepository, user: UserId, service: ServiceId, cost_basis: Usdt) -> Result<PositionView, DomainError> {
	let units = Shares::from_base_units(ledger.balance(&LedgerAccountKey::UserShares(service.clone(), user)).await?.posted);
	let (price, nav_as_of) = match nav.current(&service).await? {
		Some(v) => (v.nav, v.posted_at_unix),
		None => (Nav::SEED, 0),
	};
	let value = price.value(units)?;
	Ok(PositionView {
		service,
		units,
		nav: price,
		value,
		cost_basis,
		nav_as_of,
	})
}

/// `|new − prev| / prev > pct%`, computed on base units (saturating; a previous NAV of
/// zero makes any non-zero move "exceed", so recovering a wiped-out fund needs override).
fn nav_move_exceeds(prev: Nav, new: Nav, pct: u128) -> bool {
	let (p, n) = (prev.base_units(), new.base_units());
	p.abs_diff(n).saturating_mul(100) > p.saturating_mul(pct)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn nav_move_guard_trips_past_threshold() {
		let one = Nav::parse_decimal("1").unwrap();
		// +49% is fine, +51% trips, at the 50% threshold.
		assert!(!nav_move_exceeds(one, Nav::parse_decimal("1.49").unwrap(), MAX_NAV_MOVE_PCT));
		assert!(nav_move_exceeds(one, Nav::parse_decimal("1.51").unwrap(), MAX_NAV_MOVE_PCT));
		// A 10x fat-finger trips hard; a drop to zero trips; recovery from zero always trips.
		assert!(nav_move_exceeds(one, Nav::parse_decimal("10").unwrap(), MAX_NAV_MOVE_PCT));
		assert!(nav_move_exceeds(one, Nav::parse_decimal("0").unwrap(), MAX_NAV_MOVE_PCT));
		assert!(nav_move_exceeds(Nav::parse_decimal("0").unwrap(), one, MAX_NAV_MOVE_PCT));
	}
}
