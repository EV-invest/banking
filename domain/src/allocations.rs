//! `allocations` bounded context — external claims over the fund's value.
//!
//! An **allocation** is value in the fund that came from outside (a user or a
//! service), carrying ownership semantics borrowed from Rust: it has exactly one
//! [`owner`](Allocation::owner) (the "owned" party) and a [`sharers`](Allocation::sharers)
//! list (the "shared" list). The *same* allocation is "owned" from the owner's side
//! and "shared" from each sharer's side. A [`Party::User`] in the sharers list may
//! revoke **iff `owner == Piggybank`**; a service-sharer never can — enforced by the
//! [`UserRevocable`] specification inside [`Allocation::revoke_by_user`].
//!
//! Pure and wasm-safe (mirrors [`User`](crate::users::User)): ids are minted by the
//! application layer, no clock/no I/O. Moving the money is a **saga**: the aggregate
//! records intent + emits an event in one Postgres transaction; the outbox relay
//! then issues the TigerBeetle transfer (Write-Last). The relay maps each event to a
//! ledger operation — that orchestration lives in the adapter, never on this pure
//! aggregate.

use ev::architecture::{AggregateRoot, DomainEvent, EmitsEvents, Entity, Id, Specification};
use serde::{Deserialize, Serialize};

use crate::{
	balance::{Party, ServiceId},
	error::DomainError,
	money::Usdt,
	users::UserId,
};

/// A unique allocation id (UUID). Minted by the application layer.
pub type AllocationId = Id<AllocationTag>;
/// Phantom tag making [`AllocationId`] a distinct, incompatible identity type.
pub struct AllocationTag;

/// What an allocation *is* — fixes the owner/sharer roles and the ledger movement
/// the saga performs. Tagged for self-describing JSON in events/projections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AllocationKind {
	/// A user's funds directed at a fund service/strategy. `owner = Piggybank`,
	/// `sharers = [User]`; the user may revoke. Ledger: `Dr USER / Cr SERVICE`.
	UserStake { user: UserId, service: ServiceId },
	/// Fund capital a service has reserved and **locked** ("don't spend until the
	/// house is bought"). `owner = Piggybank`, `sharers = [Service]`. Ledger: a
	/// **pending** `Dr FUND / Cr SERVICE`, posted on settle / voided on cancel.
	ServiceReservation { service: ServiceId },
	/// Funds a service exclusively owns — a settled reservation or an instant
	/// transfer. `owner = Service`, `sharers = [Piggybank]`. Ledger: posted
	/// `Dr FUND / Cr SERVICE`.
	ServiceHolding { service: ServiceId },
}

/// Lifecycle of an allocation. `Pending` is a locked reservation (a TB pending
/// transfer); the rest are posted. `Revoked`/`Cancelled` are terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AllocationState {
	/// Reserved and locked, awaiting settle or cancel (a TB pending transfer).
	Pending,
	/// Live and counted; the value has moved (posted).
	Active,
	/// A user pulled back their stake (terminal).
	Revoked,
	/// A reservation was released back to the fund (terminal).
	Cancelled,
}

impl AllocationState {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Pending => "pending",
			Self::Active => "active",
			Self::Revoked => "revoked",
			Self::Cancelled => "cancelled",
		}
	}

	pub fn parse(raw: &str) -> Result<Self, DomainError> {
		match raw {
			"pending" => Ok(Self::Pending),
			"active" => Ok(Self::Active),
			"revoked" => Ok(Self::Revoked),
			"cancelled" => Ok(Self::Cancelled),
			other => Err(DomainError::Validation(format!("unknown allocation state: {other}"))),
		}
	}
}

/// The allocation aggregate — the saga's coordinator. Construct via the `open_*`
/// commands (each raises [`AllocationEvent::Opened`]) or [`Allocation::rehydrate`]
/// (load from the store, no events). Mutating commands accumulate one event each,
/// drained by the command handler into the event log + outbox in the same unit of
/// work.
#[derive(Debug, Clone)]
pub struct Allocation {
	id: AllocationId,
	amount: Usdt,
	owner: Party,
	sharers: Vec<Party>,
	kind: AllocationKind,
	state: AllocationState,
	pending: Vec<AllocationEvent>,
}

impl Allocation {
	/// A user directs `amount` of their claim at a service. `owner = Piggybank`,
	/// `sharers = [User]`, `Active`. Raises `Opened`.
	pub fn open_user_stake(id: AllocationId, user: UserId, service: ServiceId, amount: Usdt) -> Result<Self, DomainError> {
		Self::open(
			id,
			amount,
			Party::Piggybank,
			vec![Party::User(user)],
			AllocationKind::UserStake { user, service },
			AllocationState::Active,
		)
	}

	/// A service reserves `amount` of fund capital and locks it. `owner = Piggybank`,
	/// `sharers = [Service]`, `Pending`. Raises `Opened`. (Follow-up wiring.)
	pub fn open_service_reservation(id: AllocationId, service: ServiceId, amount: Usdt) -> Result<Self, DomainError> {
		Self::open(
			id,
			amount,
			Party::Piggybank,
			vec![Party::Service(service.clone())],
			AllocationKind::ServiceReservation { service },
			AllocationState::Pending,
		)
	}

	/// An instant transfer of `amount` of fund capital to a service, which then owns
	/// it. `owner = Service`, `sharers = [Piggybank]`, `Active`. Raises `Opened`.
	/// (Follow-up wiring.)
	pub fn open_service_transfer(id: AllocationId, service: ServiceId, amount: Usdt) -> Result<Self, DomainError> {
		Self::open(
			id,
			amount,
			Party::Service(service.clone()),
			vec![Party::Piggybank],
			AllocationKind::ServiceHolding { service },
			AllocationState::Active,
		)
	}

	fn open(id: AllocationId, amount: Usdt, owner: Party, sharers: Vec<Party>, kind: AllocationKind, state: AllocationState) -> Result<Self, DomainError> {
		if amount.is_zero() {
			return Err(DomainError::Validation("allocation amount must be positive".into()));
		}
		let mut allocation = Self {
			id,
			amount,
			owner: owner.clone(),
			sharers: sharers.clone(),
			kind: kind.clone(),
			state,
			pending: Vec::new(),
		};
		allocation.pending.push(AllocationEvent::Opened {
			allocation_id: id,
			amount,
			owner,
			sharers,
			kind,
		});
		Ok(allocation)
	}

	/// Reconstitute from the store. Raises no events.
	pub fn rehydrate(id: AllocationId, amount: Usdt, owner: Party, sharers: Vec<Party>, kind: AllocationKind, state: AllocationState) -> Self {
		Self {
			id,
			amount,
			owner,
			sharers,
			kind,
			state,
			pending: Vec::new(),
		}
	}

	/// Revoke a user's stake, returning the value to the user. Permitted **iff** the
	/// [`UserRevocable`] policy holds — `owner == Piggybank` and `user` is the sole
	/// sharer (so a service-sharer can never revoke, and a user can't revoke another
	/// user's stake). Idempotent: a no-op if already revoked.
	///
	/// This is the *stateful* rule — the persistence adapter calls it under a row
	/// lock (`SELECT … FOR UPDATE`), so it is the single authority on validity; the
	/// gRPC boundary only does the cheap "are you this user?" check.
	pub fn revoke_by_user(&mut self, user: UserId) -> Result<(), DomainError> {
		if self.state == AllocationState::Revoked {
			return Ok(());
		}
		if self.state != AllocationState::Active {
			return Err(DomainError::Conflict(format!("allocation is {}, not revocable", self.state.as_str())));
		}
		if !UserRevocable(user).holds(&*self) {
			return Err(DomainError::Forbidden("only the staking user may revoke, and only while the fund owns it".into()));
		}
		let AllocationKind::UserStake { service, .. } = &self.kind else {
			return Err(DomainError::Forbidden("not a user stake".into()));
		};
		let service = service.clone();
		self.state = AllocationState::Revoked;
		self.pending.push(AllocationEvent::Revoked {
			allocation_id: self.id,
			amount: self.amount,
			user,
			service,
		});
		Ok(())
	}

	/// Settle a pending reservation: the service now owns the funds (the reservation
	/// becomes a holding) and the saga posts the pending transfer. (Follow-up wiring.)
	pub fn settle(&mut self) -> Result<(), DomainError> {
		let AllocationKind::ServiceReservation { service } = &self.kind else {
			return Err(DomainError::Conflict("only a reservation can be settled".into()));
		};
		if self.state != AllocationState::Pending {
			return Err(DomainError::Conflict(format!("reservation is {}, not settleable", self.state.as_str())));
		}
		let service = service.clone();
		self.owner = Party::Service(service.clone());
		self.sharers = vec![Party::Piggybank];
		self.kind = AllocationKind::ServiceHolding { service: service.clone() };
		self.state = AllocationState::Active;
		self.pending.push(AllocationEvent::Settled {
			allocation_id: self.id,
			amount: self.amount,
			service,
		});
		Ok(())
	}

	/// Cancel a pending reservation: the saga voids the pending transfer, returning
	/// the capital to the fund. (Follow-up wiring.)
	pub fn cancel(&mut self) -> Result<(), DomainError> {
		let AllocationKind::ServiceReservation { service } = &self.kind else {
			return Err(DomainError::Conflict("only a reservation can be cancelled".into()));
		};
		if self.state != AllocationState::Pending {
			return Err(DomainError::Conflict(format!("reservation is {}, not cancellable", self.state.as_str())));
		}
		let service = service.clone();
		self.state = AllocationState::Cancelled;
		self.pending.push(AllocationEvent::Cancelled {
			allocation_id: self.id,
			amount: self.amount,
			service,
		});
		Ok(())
	}

	pub fn id(&self) -> AllocationId {
		self.id
	}

	pub fn amount(&self) -> Usdt {
		self.amount
	}

	pub fn owner(&self) -> &Party {
		&self.owner
	}

	pub fn sharers(&self) -> &[Party] {
		&self.sharers
	}

	pub fn kind(&self) -> &AllocationKind {
		&self.kind
	}

	pub fn state(&self) -> AllocationState {
		self.state
	}
}

impl Entity for Allocation {
	type Id = AllocationId;

	fn id(&self) -> AllocationId {
		self.id
	}
}

impl AggregateRoot for Allocation {
	const NAME: &'static str = "allocation";
}

/// The revocation policy as a composable [`Specification`]: the fund owns it **and**
/// `user` is the sole sharer. Expressed via the kernel's spec combinators so the rule
/// reads as the sentence it enforces.
pub struct UserRevocable(pub UserId);

/// Facts raised by the [`Allocation`] aggregate. Each carries the saga-relevant data
/// (amount, parties) so the relay maps an event to a ledger operation with no extra
/// read. Internally tagged so the stored JSON is self-describing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AllocationEvent {
	Opened {
		allocation_id: AllocationId,
		amount: Usdt,
		owner: Party,
		sharers: Vec<Party>,
		kind: AllocationKind,
	},
	Revoked {
		allocation_id: AllocationId,
		amount: Usdt,
		user: UserId,
		service: ServiceId,
	},
	Settled {
		allocation_id: AllocationId,
		amount: Usdt,
		service: ServiceId,
	},
	Cancelled {
		allocation_id: AllocationId,
		amount: Usdt,
		service: ServiceId,
	},
}
struct OwnedByPiggybank;
impl Specification<Allocation> for OwnedByPiggybank {
	fn holds(&self, allocation: &Allocation) -> bool {
		allocation.owner.is_piggybank()
	}
}

struct SoleUserSharer(UserId);
impl Specification<Allocation> for SoleUserSharer {
	fn holds(&self, allocation: &Allocation) -> bool {
		allocation.sharers.len() == 1 && allocation.sharers[0] == Party::User(self.0)
	}
}

impl Specification<Allocation> for UserRevocable {
	fn holds(&self, allocation: &Allocation) -> bool {
		OwnedByPiggybank.and(SoleUserSharer(self.0)).holds(allocation)
	}
}

impl DomainEvent for AllocationEvent {
	const KIND: &'static str = "allocations";
}

impl EmitsEvents for Allocation {
	type Event = AllocationEvent;

	fn drain_events(&mut self) -> Vec<AllocationEvent> {
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
		ServiceId::parse("real-estate").unwrap()
	}
	fn amount() -> Usdt {
		Usdt::parse_decimal("1000").unwrap()
	}

	#[test]
	fn open_user_stake_sets_roles_and_emits_opened() {
		let mut a = Allocation::open_user_stake(AllocationId::new(), user(), svc(), amount()).unwrap();
		assert_eq!(*a.owner(), Party::Piggybank);
		assert_eq!(a.state(), AllocationState::Active);
		let events = a.drain_events();
		assert_eq!(events.len(), 1);
		assert!(matches!(events[0], AllocationEvent::Opened { .. }));
		assert!(a.drain_events().is_empty());
	}

	#[test]
	fn zero_amount_is_rejected() {
		assert!(Allocation::open_user_stake(AllocationId::new(), user(), svc(), Usdt::ZERO).is_err());
	}

	#[test]
	fn the_staking_user_may_revoke() {
		let u = user();
		let mut a = Allocation::open_user_stake(AllocationId::new(), u, svc(), amount()).unwrap();
		a.drain_events();
		a.revoke_by_user(u).unwrap();
		assert_eq!(a.state(), AllocationState::Revoked);
		let events = a.drain_events();
		assert!(matches!(events[0], AllocationEvent::Revoked { .. }));
	}

	#[test]
	fn another_user_may_not_revoke() {
		let mut a = Allocation::open_user_stake(AllocationId::new(), user(), svc(), amount()).unwrap();
		a.drain_events();
		let err = a.revoke_by_user(user()).unwrap_err();
		assert!(matches!(err, DomainError::Forbidden(_)));
		assert_eq!(a.state(), AllocationState::Active);
	}

	#[test]
	fn revoke_is_idempotent() {
		let u = user();
		let mut a = Allocation::open_user_stake(AllocationId::new(), u, svc(), amount()).unwrap();
		a.drain_events();
		a.revoke_by_user(u).unwrap();
		a.drain_events();
		// Second revoke: no error, no event.
		a.revoke_by_user(u).unwrap();
		assert!(a.drain_events().is_empty());
	}

	#[test]
	fn a_service_sharer_is_not_user_revocable() {
		// A reservation is owned by Piggybank but shared with a Service — no user can
		// revoke it (and it isn't Active, but the policy must fail regardless).
		let a = Allocation::open_service_reservation(AllocationId::new(), svc(), amount()).unwrap();
		assert!(!UserRevocable(user()).holds(&a));
	}

	#[test]
	fn reservation_settles_into_a_service_holding() {
		let mut a = Allocation::open_service_reservation(AllocationId::new(), svc(), amount()).unwrap();
		a.drain_events();
		assert_eq!(a.state(), AllocationState::Pending);
		a.settle().unwrap();
		assert_eq!(a.state(), AllocationState::Active);
		assert_eq!(*a.owner(), Party::Service(svc()));
		assert!(matches!(a.kind(), AllocationKind::ServiceHolding { .. }));
		assert!(matches!(a.drain_events()[0], AllocationEvent::Settled { .. }));
		// A settled (now Active) holding can no longer be settled or cancelled.
		assert!(a.settle().is_err());
		assert!(a.cancel().is_err());
	}

	#[test]
	fn reservation_cancels_back_to_fund() {
		let mut a = Allocation::open_service_reservation(AllocationId::new(), svc(), amount()).unwrap();
		a.drain_events();
		a.cancel().unwrap();
		assert_eq!(a.state(), AllocationState::Cancelled);
		assert!(matches!(a.drain_events()[0], AllocationEvent::Cancelled { .. }));
	}

	#[test]
	fn event_round_trips_through_json() {
		let mut a = Allocation::open_user_stake(AllocationId::new(), user(), svc(), amount()).unwrap();
		let event = a.drain_events().pop().unwrap();
		let json = serde_json::to_string(&event).unwrap();
		let back: AllocationEvent = serde_json::from_str(&json).unwrap();
		assert!(matches!(back, AllocationEvent::Opened { .. }));
	}
}
