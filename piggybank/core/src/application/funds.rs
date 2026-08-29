//! Fund (share) use cases — the service currency.
//!
//! NAV is **derived**, not posted directly: an operator posts a fund's total AUM and the
//! handler reads `units_outstanding` live from TigerBeetle to compute
//! `NAV = AUM / units_outstanding` (frozen until the next mark). Subscribe/redeem
//! (later slices) deal on the latest mark — a deliberate *backward-pricing* tradeoff,
//! guarded by a **staleness** check; the operator post is guarded by a **move** check,
//! because the AUM input is the most dangerous seam in the system ("trusted" ≠ "safe").

use domain::{
	balance::{LedgerAccountKey, ServiceId, ValuationId},
	error::DomainError,
	money::{Nav, Shares, Usdt},
	redemptions::{Redemption, RedemptionId, RedemptionState},
	subscriptions::{Subscription, SubscriptionId},
	users::UserId,
};
use tokio::sync::Notify;

use crate::{
	application::allocations as allocations_app,
	ports::{
		FundPositionReader, RedemptionRepository, SubscriptionRepository,
		allocations::AllocationRegistry,
		ledger::Ledger,
		nav::{NavMarks, Valuation},
	},
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
	/// The **settled** supply — the denominator NAV is derived against.
	pub units_outstanding: Shares,
	/// The allocation's authorised unit supply.
	pub unit_cap: Shares,
	/// Units still issuable, measured the way [`subscribe`] measures them (settled plus
	/// in-flight). Reported rather than left to the caller to subtract, so a screen can
	/// never offer headroom the subscribe gate would then refuse.
	pub remaining_capacity: Shares,
	/// Unix seconds of the latest mark (0 = never marked / seed).
	pub posted_at: i64,
	pub stale: bool,
}

/// The driven ports a dealing use-case borrows: the registry it gates on, the ledger its
/// Read-First checks read, the marks it prices at, and the relay it nudges once the
/// control-plane commit lands. Bundled so a use-case's own parameters are its *request* —
/// who, which fund, how much, as of when — rather than the wiring the composition root
/// injects. A plain borrow-holder: it owns nothing, decides nothing, and outlives nothing.
pub struct FundPorts<'a> {
	/// The registry of investable products — the gate every deal resolves `service` through.
	pub allocations: &'a dyn AllocationRegistry,
	/// The money gateway (TigerBeetle): the authoritative balances the Read-First checks read.
	pub ledger: &'a dyn Ledger,
	/// The valuation marks a deal is priced at, staleness guard included.
	pub nav: &'a dyn NavMarks,
	/// Nudged after the commit so the outbox relay moves money promptly.
	pub relay: &'a Notify,
}

/// The current NAV plus whether it is fresh enough to deal on (`now − posted_at ≤
/// MAX_NAV_AGE_SECS`). A fund with no mark yet uses the seed NAV and is always fresh
/// (nothing to be stale against). Subscribe/redeem call this before pricing.
pub async fn dealing_nav(nav: &dyn NavMarks, service: &ServiceId, now_unix: i64) -> Result<Nav, DomainError> {
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
///
/// The **registry gate runs first**, before any balance read or pricing: `service` must
/// be a registered, `open` allocation. It is what stops a user from minting a fund out
/// of an arbitrary slug — previously the only check was the slug's shape, and a service
/// with no valuation bootstrapped silently at the seed NAV.
///
/// The **supply gate** runs second, once the mint has been priced and is therefore
/// known: `issued + minting` must fit the allocation's unit cap. Like the cash check
/// above it is Read-First — and unlike the cash check it has no TigerBeetle backstop
/// behind it, because a ledger can refuse to go below zero but has no notion of a
/// ceiling. Two consequences, both deliberate:
///
/// * Concurrent subscribes can each read the same `issued` and both pass, so the cap can
///   be overshot by what is in flight at that instant.
/// * `issued` is read from the ledger, which the relay writes *after* the control-plane
///   commit — so a subscription committed moments ago may not be counted yet.
///
/// The cap is therefore an **issuance gate, not an invariant**: it reliably stops a fund
/// from running away, and does not promise the last unit is exact. Making it exact would
/// mean reconstructing the outstanding supply in Postgres — a second source of truth for
/// a figure TigerBeetle already owns, which is the trade this architecture refuses
/// everywhere else.
pub async fn subscribe(ports: &FundPorts<'_>, subscriptions: &dyn SubscriptionRepository, user: UserId, service: ServiceId, cash: Usdt, now_unix: i64) -> Result<Subscription, DomainError> {
	let allocation = allocations_app::require_subscribable(ports.allocations, &service).await?;
	let claim = ports.ledger.balance(&LedgerAccountKey::UserClaim(user)).await?;
	if Usdt::from_base_units(claim.available()) < cash {
		return Err(DomainError::Validation("insufficient available balance to subscribe".into()));
	}
	let price = dealing_nav(ports.nav, &service, now_unix).await?;
	allocation.ensure_capacity(issued_units(ports.ledger, &service).await?, Shares::from_cash(cash, price)?)?;
	let mut subscription = Subscription::open(SubscriptionId::new(), user, service, cash, price)?;
	subscriptions.open(&mut subscription).await?;
	ports.relay.notify_one();
	Ok(subscription)
}

/// Units the ledger considers issued for `service` — settled **plus in-flight inflow**.
///
/// Counting `pending` is the conservative direction for a ceiling: an unsettled mint is
/// supply that is on its way out, and treating it as absent would hand the same headroom
/// to two subscriptions. Pending *burns* (`locked`) are deliberately not subtracted —
/// those units still exist until the burn settles, and a queued redemption that is later
/// cancelled would otherwise have briefly re-opened capacity that was never free.
async fn issued_units(ledger: &dyn Ledger, service: &ServiceId) -> Result<Shares, DomainError> {
	let balance = ledger.balance(&LedgerAccountKey::SharesOutstanding(service.clone())).await?;
	Ok(Shares::from_base_units(balance.posted.saturating_add(balance.pending)))
}

/// A user redeems `units` of `service` back to cash. Read-First confirms the user holds
/// the units (TigerBeetle's flag is the over-redeem backstop); the staleness guard
/// refuses to deal on a drifted mark. The redemption is **accepted and queued**: the
/// relay reserves a pending burn now, and the cash is priced + paid at **settle**. If the
/// fund's claim can already cover the payout, this settles immediately via a **separate**
/// command (never co-emitting `Requested`+`Settled`, which would race the burn reserve);
/// otherwise it stays `Queued` for an operator `settle_redemption` once the fund tops up.
///
/// The registry gate here is the **laxer** one: a `closed` allocation still redeems, so
/// winding a product down never traps an investor's units inside it.
pub async fn request_redemption(
	ports: &FundPorts<'_>,
	redemptions: &dyn RedemptionRepository,
	user: UserId,
	service: ServiceId,
	units: Shares,
	now_unix: i64,
) -> Result<Redemption, DomainError> {
	allocations_app::require_redeemable(ports.allocations, &service).await?;
	let holding = ports.ledger.balance(&LedgerAccountKey::UserShares(service.clone(), user)).await?;
	if Shares::from_base_units(holding.available()) < units {
		return Err(DomainError::Validation("insufficient units to redeem".into()));
	}
	// Fresh NAV (staleness guard) — also the auto-settle liquidity estimate.
	let price = dealing_nav(ports.nav, &service, now_unix).await?;
	let cash_out = price.value(units)?;
	let mut redemption = Redemption::request(RedemptionId::new(), user, service.clone(), units)?;
	redemptions.open(&mut redemption).await?;
	ports.relay.notify_one();
	// Accept-and-queue: settle now (as a separate command) iff the fund's claim can cover
	// the payout; else leave it queued for the treasury worker.
	let fund = ports.ledger.balance(&LedgerAccountKey::ServiceClaim(service)).await?;
	if Usdt::from_base_units(fund.available()) >= cash_out {
		// The settle can lose a race — to the relay's subscribe projection (rolled back,
		// stays queued for the operator) or to a concurrent cancel/fail. The redemption
		// was accepted either way, so a `Conflict` reports its actual current state
		// rather than surfacing an error for an already-opened redemption.
		return match settle_redemption(redemptions, ports.nav, ports.relay, redemption.id(), now_unix).await {
			Err(DomainError::Conflict(_)) => redemptions.find_by_id(redemption.id()).await?.ok_or_else(|| DomainError::NotFound {
				entity: "redemption",
				id: redemption.id().to_string(),
			}),
			settled => settled,
		};
	}
	Ok(redemption)
}

/// Settle a queued redemption (the auto follow-on, or an operator once the fund is
/// liquid): prices the cash at the **settle-time** NAV (`units × NAV`) and pays it. The
/// relay posts the burn then the payout, guarded by a Read-First check on the fund claim.
/// The cost-basis reduction divides by the position's own projection-tracked units inside
/// the locked settle tx — not a live TB holding, which lags the async burn — so back-to-back
/// settles compound deterministically (BANK-MONEY-3), and it applies exactly once per
/// redemption (see [`crate::infrastructure::redemptions`]).
pub async fn settle_redemption(redemptions: &dyn RedemptionRepository, nav: &dyn NavMarks, relay: &Notify, id: RedemptionId, now_unix: i64) -> Result<Redemption, DomainError> {
	let existing = redemptions.find_by_id(id).await?.ok_or_else(|| DomainError::NotFound {
		entity: "redemption",
		id: id.to_string(),
	})?;
	// Completed is terminal, so this unlocked read is safe to short-circuit on: the
	// documented-idempotent retry must not re-check NAV freshness (a mark gone stale
	// since the real settle would fail it). Concurrent settles that both pass this
	// pre-check serialize on the repo's row lock, where the same guard is authoritative.
	if existing.state() == RedemptionState::Completed {
		return Ok(existing);
	}
	let price = dealing_nav(nav, existing.service(), now_unix).await?;
	let redemption = redemptions.settle(id, price).await?;
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
pub async fn get_position(positions: &dyn FundPositionReader, ledger: &dyn Ledger, nav: &dyn NavMarks, user: UserId, service: ServiceId) -> Result<PositionView, DomainError> {
	let cost_basis = positions.find(user, &service).await?.map(|p| p.cost_basis).unwrap_or(Usdt::ZERO);
	build_position_view(ledger, nav, user, service, cost_basis).await
}

/// All of the caller's fund positions with a non-zero unit balance.
pub async fn list_positions(positions: &dyn FundPositionReader, ledger: &dyn Ledger, nav: &dyn NavMarks, user: UserId) -> Result<Vec<PositionView>, DomainError> {
	let mut out = Vec::new();
	for position in positions.list(user).await? {
		let view = build_position_view(ledger, nav, user, position.service, position.cost_basis).await?;
		if !view.units.is_zero() {
			out.push(view);
		}
	}
	Ok(out)
}

/// The current NAV + freshness for a fund (the seed NAV when never marked), plus the
/// supply headroom left against its allocation's cap. Gated on the allocation existing,
/// for the same reason the valuation post is: a price quoted for a service no registry
/// entry backs is a price for a fund that does not exist.
pub async fn fund_nav_view(allocations: &dyn AllocationRegistry, nav: &dyn NavMarks, ledger: &dyn Ledger, service: ServiceId, now_unix: i64) -> Result<FundNavView, DomainError> {
	let allocation = allocations_app::get(allocations, &service).await?;
	let balance = ledger.balance(&LedgerAccountKey::SharesOutstanding(service.clone())).await?;
	let units_outstanding = Shares::from_base_units(balance.posted);
	let remaining_capacity = allocation.remaining_capacity(Shares::from_base_units(balance.posted.saturating_add(balance.pending)));
	let (unit_cap, current) = (allocation.unit_cap(), nav.current(&service).await?);
	Ok(match current {
		Some(v) => FundNavView {
			service,
			nav: v.nav,
			aum: Some(v.aum),
			units_outstanding,
			unit_cap,
			remaining_capacity,
			posted_at: v.posted_at_unix,
			stale: now_unix.saturating_sub(v.posted_at_unix) > MAX_NAV_AGE_SECS,
		},
		None => FundNavView {
			service,
			nav: Nav::SEED,
			aum: None,
			units_outstanding,
			unit_cap,
			remaining_capacity,
			posted_at: 0,
			stale: false,
		},
	})
}
/// Operator posts a fund's total AUM; NAV is derived (`AUM / units_outstanding`, read
/// live from TigerBeetle). Rejects zero units (NAV undefined) and — unless `force` — a
/// move beyond [`MAX_NAV_MOVE_PCT`] vs the last mark. Records the mark (with `posted_by`)
/// and returns it.
///
/// Gated on the allocation *existing* (any state — a closed product still gets marked so
/// queued redemptions price correctly). Without this an AUM post would write a valuation
/// history for a service no registry entry backs — the second way a phantom fund used to
/// come into being.
pub async fn post_fund_valuation(
	allocations: &dyn AllocationRegistry,
	nav: &dyn NavMarks,
	ledger: &dyn Ledger,
	service: ServiceId,
	aum: Usdt,
	posted_by: &str,
	force: bool,
) -> Result<Valuation, DomainError> {
	allocations_app::get(allocations, &service).await?;
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
	let posted_at_unix = nav.record(ValuationId::new(), &service, aum, units, derived, posted_by).await?;
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
async fn build_position_view(ledger: &dyn Ledger, nav: &dyn NavMarks, user: UserId, service: ServiceId, cost_basis: Usdt) -> Result<PositionView, DomainError> {
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
