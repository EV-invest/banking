//! `subscriptions` bounded context — a user buying fund units (shares).
//!
//! Subscribing directs `cash` of a user's free claim into a fund (a [`ServiceId`]) and
//! mints `units = cash / NAV` of the **service currency** at the current price. It is
//! the safe direction (value stays in the system, just changes form), so — like a
//! deposit — it is **synchronous**: one immutable record, no saga. The relay posts two
//! legs: the cash `Dr UserClaim / Cr ServiceClaim`, then the units mint
//! `Dr UserShares / Cr SharesOutstanding`. Cash-leg first, so a stale-but-insufficient
//! balance parks before any units are minted — never units without cash.
//!
//! Pure and wasm-safe (mirrors [`Withdrawal`](crate::withdrawals::Withdrawal)): ids are
//! minted by the application layer, no clock, no I/O. NAV is supplied by the caller
//! (read from the valuation control plane), never derived here.

use ev::architecture::{AggregateRoot, DomainEvent, EmitsEvents, Entity, Id};
use serde::{Deserialize, Serialize};

use crate::{
	balance::ServiceId,
	error::DomainError,
	money::{Nav, Shares, Usdt},
	users::UserId,
};

/// A unique subscription id (UUID). Minted by the application layer.
pub type SubscriptionId = Id<SubscriptionTag>;
/// Phantom tag making [`SubscriptionId`] a distinct, incompatible identity type.
pub struct SubscriptionTag;

/// The subscription aggregate — an immutable record of one mint. Construct via
/// [`Subscription::open`] (raises [`SubscriptionEvent::Subscribed`]) or
/// [`Subscription::rehydrate`] (load from the store, no events).
#[derive(Clone, Debug)]
pub struct Subscription {
	id: SubscriptionId,
	user: UserId,
	service: ServiceId,
	cash: Usdt,
	nav: Nav,
	units: Shares,
	pending: Vec<SubscriptionEvent>,
}

impl Subscription {
	/// Subscribe `cash` into `service` at `nav`, minting `floor(cash / nav)` units.
	/// Rejects a zero `cash` and a `cash` too small to buy a single base-unit of share
	/// (`units == 0`). Raises `Subscribed`.
	pub fn open(id: SubscriptionId, user: UserId, service: ServiceId, cash: Usdt, nav: Nav) -> Result<Self, DomainError> {
		if cash.is_zero() {
			return Err(DomainError::Validation("subscription amount must be positive".into()));
		}
		let units = Shares::from_cash(cash, nav)?;
		if units.is_zero() {
			return Err(DomainError::Validation("amount too small to buy a share at the current nav".into()));
		}
		let mut subscription = Self {
			id,
			user,
			service: service.clone(),
			cash,
			nav,
			units,
			pending: Vec::new(),
		};
		subscription.pending.push(SubscriptionEvent::Subscribed {
			subscription_id: id,
			user,
			service,
			cash,
			nav,
			units,
		});
		Ok(subscription)
	}

	/// Reconstitute from the store. Raises no events.
	pub fn rehydrate(id: SubscriptionId, user: UserId, service: ServiceId, cash: Usdt, nav: Nav, units: Shares) -> Self {
		Self {
			id,
			user,
			service,
			cash,
			nav,
			units,
			pending: Vec::new(),
		}
	}

	pub fn id(&self) -> SubscriptionId {
		self.id
	}

	pub fn user(&self) -> UserId {
		self.user
	}

	pub fn service(&self) -> &ServiceId {
		&self.service
	}

	pub fn cash(&self) -> Usdt {
		self.cash
	}

	pub fn nav(&self) -> Nav {
		self.nav
	}

	pub fn units(&self) -> Shares {
		self.units
	}
}

impl Entity for Subscription {
	type Id = SubscriptionId;

	fn id(&self) -> SubscriptionId {
		self.id
	}
}

impl AggregateRoot for Subscription {
	const NAME: &'static str = "subscription";
}

/// Facts raised by the [`Subscription`] aggregate. `Subscribed` carries everything the
/// relay needs to post both legs (cash + mint) with no extra read. Internally tagged so
/// the stored JSON is self-describing.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SubscriptionEvent {
	/// Units minted (relay: `Dr UserClaim / Cr ServiceClaim` for `cash`, then
	/// `Dr UserShares / Cr SharesOutstanding` for `units`).
	Subscribed {
		subscription_id: SubscriptionId,
		user: UserId,
		service: ServiceId,
		cash: Usdt,
		nav: Nav,
		units: Shares,
	},
}

impl DomainEvent for SubscriptionEvent {
	const KIND: &'static str = "subscriptions";
}

impl EmitsEvents for Subscription {
	type Event = SubscriptionEvent;

	fn drain_events(&mut self) -> Vec<SubscriptionEvent> {
		core::mem::take(&mut self.pending)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn user() -> UserId {
		UserId::new()
	}
	fn svc() -> ServiceId {
		ServiceId::parse("trading").unwrap()
	}

	#[test]
	fn open_mints_units_at_nav_and_emits_subscribed() {
		// 300 USDT at NAV 1.5 → 200 units.
		let mut s = Subscription::open(SubscriptionId::new(), user(), svc(), Usdt::parse_decimal("300").unwrap(), Nav::parse_decimal("1.5").unwrap()).unwrap();
		assert_eq!(s.units(), Shares::parse_decimal("200").unwrap());
		let events = s.drain_events();
		assert_eq!(events.len(), 1);
		assert!(matches!(events[0], SubscriptionEvent::Subscribed { .. }));
		assert!(s.drain_events().is_empty());
	}

	#[test]
	fn seed_nav_mints_one_for_one() {
		let s = Subscription::open(SubscriptionId::new(), user(), svc(), Usdt::parse_decimal("100").unwrap(), Nav::SEED).unwrap();
		assert_eq!(s.units(), Shares::parse_decimal("100").unwrap());
	}

	#[test]
	fn zero_cash_and_dust_are_rejected() {
		assert!(Subscription::open(SubscriptionId::new(), user(), svc(), Usdt::ZERO, Nav::SEED).is_err());
		// 2 base units of USDT at NAV 3 floors to 0 units → rejected.
		assert!(Subscription::open(SubscriptionId::new(), user(), svc(), Usdt::from_base_units(2), Nav::parse_decimal("3").unwrap()).is_err());
	}

	#[test]
	fn event_round_trips_through_json() {
		let mut s = Subscription::open(SubscriptionId::new(), user(), svc(), Usdt::parse_decimal("100").unwrap(), Nav::SEED).unwrap();
		let event = s.drain_events().pop().unwrap();
		let json = serde_json::to_string(&event).unwrap();
		let back: SubscriptionEvent = serde_json::from_str(&json).unwrap();
		assert!(matches!(back, SubscriptionEvent::Subscribed { .. }));
	}
}
