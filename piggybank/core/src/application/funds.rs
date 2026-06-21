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
	subscriptions::{Subscription, SubscriptionId},
	users::UserId,
};
use tokio::sync::Notify;
use uuid::Uuid;

use crate::ports::{
	SubscriptionRepository,
	ledger::Ledger,
	nav::{NavRepository, Valuation},
};

/// A derived NAV that jumps more than this (percent) from the previous mark is rejected
/// unless the operator passes an override — the fat-finger guard on the AUM trust seam.
pub const MAX_NAV_MOVE_PCT: u128 = 50;
/// A mark older than this (seconds) is stale; subscribe/redeem refuse to deal on it
/// rather than price off a drifted NAV (the backward-pricing arbitrage guard). 24h for v1.
pub const MAX_NAV_AGE_SECS: i64 = 24 * 60 * 60;

/// The current NAV for `service`: the latest mark, or the bootstrap seed (1.0) when the
/// fund has no units yet (the first subscription mints at seed, establishing units).
pub async fn current_nav(nav: &dyn NavRepository, service: &ServiceId) -> Result<Nav, DomainError> {
	Ok(nav.current(service).await?.map(|v| v.nav).unwrap_or(Nav::SEED))
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
