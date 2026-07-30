//! `allocations` bounded context — the registry of investable products.
//!
//! An [`Allocation`] is the control-plane record that makes a [`ServiceId`] investable.
//! Before it existed, a fund came into being the moment *any* user subscribed to an
//! arbitrary slug: `Subscribe` validated only the slug's shape, and a missing valuation
//! silently bootstrapped at the seed NAV. A typo therefore minted a real fund holding
//! real money and answering to no operator. The registry closes that: the subscribe
//! path resolves its service here first, so an investable product exists only because
//! someone holding `AllocationManage` registered and opened it.
//!
//! This aggregate carries **no money**. Units, NAV, cost basis and cash live in the
//! `subscriptions` / `redemptions` / `balance` contexts, keyed by the same [`ServiceId`]
//! — the registry owns identity, presentation and lifecycle only. Its events are audit
//! facts (`event_log`), never relayed to the ledger.
//!
//! Pure and wasm-safe (mirrors [`Withdrawal`](crate::withdrawals::Withdrawal)): ids are
//! minted by the application layer, no clock, no I/O. Timestamps are DB-stamped and
//! surfaced by the read port, so they never enter the aggregate.

use ev::architecture::{AggregateRoot, DomainEvent, EmitsEvents, Entity, Id};
use serde::{Deserialize, Serialize};

use crate::{balance::ServiceId, error::DomainError};

/// The longest accepted display title.
const MAX_TITLE_LEN: usize = 120;
/// The longest accepted catalog one-liner.
const MAX_SUMMARY_LEN: usize = 280;

/// A unique allocation id (UUID). Minted by the application layer.
///
/// A surrogate: the allocation's *natural* key is its [`ServiceId`] slug, which is what
/// every other context and the whole wire surface uses. The slug is a `String` newtype
/// and so cannot satisfy `Identifier: Copy`, hence the UUID — it exists to satisfy the
/// `Entity`/event-log plumbing (`event_log.aggregate_id`), never to be an alternative
/// public handle. Look allocations up by [`ServiceId`].
pub type AllocationId = Id<AllocationTag>;
/// Phantom tag making [`AllocationId`] a distinct, incompatible identity type.
pub struct AllocationTag;

/// Lifecycle of an investable product.
///
/// `Closed` deliberately still permits redemptions: an operator winding a product down
/// must not be able to strand an investor's money inside it. Only the *new money*
/// direction is gated.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AllocationState {
	/// Registered but accepting no money — the product is being prepared. Hidden from
	/// the default catalog.
	Draft,
	/// Live: accepts subscriptions and redemptions.
	Open,
	/// Wound down: redemptions still settle, new subscriptions are refused.
	Closed,
}

impl AllocationState {
	/// The stored/wire discriminant. Keep byte-identical with
	/// `evbanking_contracts::allocation::state` (`allocation_state_strings_are_canonical`
	/// guards this side).
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Draft => "draft",
			Self::Open => "open",
			Self::Closed => "closed",
		}
	}

	/// Parse the stored/wire form. An unrecognized value is an error rather than a
	/// silent default, so a corrupt row never quietly opens a product for business.
	pub fn parse(raw: &str) -> Result<Self, DomainError> {
		match raw {
			"draft" => Ok(Self::Draft),
			"open" => Ok(Self::Open),
			"closed" => Ok(Self::Closed),
			other => Err(DomainError::Validation(format!("unknown allocation state: {other}"))),
		}
	}
}

/// The allocation aggregate — one investable product's registry entry. Construct via
/// [`Allocation::register`] (raises [`AllocationEvent::Registered`]) or
/// [`Allocation::rehydrate`] (load from the store, no events).
#[derive(Clone, Debug)]
pub struct Allocation {
	id: AllocationId,
	service: ServiceId,
	title: String,
	summary: String,
	state: AllocationState,
	pending: Vec<AllocationEvent>,
}

impl Allocation {
	/// Register `service` as an investable product, in [`AllocationState::Draft`] — a
	/// registration never opens for business in the same step, so listing and funding
	/// stay separate operator decisions. Raises `Registered`.
	pub fn register(id: AllocationId, service: ServiceId, title: &str, summary: &str) -> Result<Self, DomainError> {
		let title = validate_title(title)?;
		let summary = validate_summary(summary)?;
		let mut allocation = Self {
			id,
			service: service.clone(),
			title: title.clone(),
			summary: summary.clone(),
			state: AllocationState::Draft,
			pending: Vec::new(),
		};
		allocation.pending.push(AllocationEvent::Registered {
			allocation_id: id,
			service,
			title,
			summary,
		});
		Ok(allocation)
	}

	/// Reconstitute from the store. Raises no events.
	pub fn rehydrate(id: AllocationId, service: ServiceId, title: String, summary: String, state: AllocationState) -> Self {
		Self {
			id,
			service,
			title,
			summary,
			state,
			pending: Vec::new(),
		}
	}

	/// Replace the presentation fields. Identity and state are untouched — an operator
	/// may fix a title on a live product without disturbing money flow. Raises
	/// `DetailsUpdated`.
	pub fn update_details(&mut self, title: &str, summary: &str) -> Result<(), DomainError> {
		self.title = validate_title(title)?;
		self.summary = validate_summary(summary)?;
		self.pending.push(AllocationEvent::DetailsUpdated {
			allocation_id: self.id,
			service: self.service.clone(),
			title: self.title.clone(),
			summary: self.summary.clone(),
		});
		Ok(())
	}

	/// Open for business (from `Draft` or `Closed`). Idempotent: already `Open` is a
	/// no-op that raises nothing, so a retried operator command can't spam the log.
	pub fn open(&mut self) {
		if self.state == AllocationState::Open {
			return;
		}
		self.state = AllocationState::Open;
		self.pending.push(AllocationEvent::Opened {
			allocation_id: self.id,
			service: self.service.clone(),
		});
	}

	/// Wind the product down. Redemptions keep working ([`Self::ensure_redeemable`]);
	/// only new subscriptions stop. Idempotent, and legal from `Draft` too (retiring a
	/// product that never opened).
	pub fn close(&mut self) {
		if self.state == AllocationState::Closed {
			return;
		}
		self.state = AllocationState::Closed;
		self.pending.push(AllocationEvent::Closed {
			allocation_id: self.id,
			service: self.service.clone(),
		});
	}

	/// The gate the subscribe path runs: only an `Open` allocation takes new money.
	pub fn ensure_subscribable(&self) -> Result<(), DomainError> {
		match self.state {
			AllocationState::Open => Ok(()),
			AllocationState::Draft => Err(DomainError::Validation(format!("allocation '{}' is not open for subscriptions yet", self.service))),
			AllocationState::Closed => Err(DomainError::Validation(format!("allocation '{}' is closed to new subscriptions", self.service))),
		}
	}

	/// The gate the redeem path runs. A `Closed` allocation still lets investors out —
	/// refusing here would lock real units inside a wound-down product. Only a `Draft`
	/// is refused, and only because it can hold no units to begin with.
	pub fn ensure_redeemable(&self) -> Result<(), DomainError> {
		match self.state {
			AllocationState::Open | AllocationState::Closed => Ok(()),
			AllocationState::Draft => Err(DomainError::Validation(format!("allocation '{}' has never been open", self.service))),
		}
	}

	/// Whether this allocation belongs in the default (investor-facing) catalog.
	pub fn is_listed(&self) -> bool {
		self.state == AllocationState::Open
	}

	pub fn id(&self) -> AllocationId {
		self.id
	}

	pub fn service(&self) -> &ServiceId {
		&self.service
	}

	pub fn title(&self) -> &str {
		&self.title
	}

	pub fn summary(&self) -> &str {
		&self.summary
	}

	pub fn state(&self) -> AllocationState {
		self.state
	}
}

fn validate_title(raw: &str) -> Result<String, DomainError> {
	let title = raw.trim();
	if title.is_empty() {
		return Err(DomainError::Validation("allocation title must not be empty".into()));
	}
	if title.chars().count() > MAX_TITLE_LEN {
		return Err(DomainError::Validation(format!("allocation title must be at most {MAX_TITLE_LEN} characters")));
	}
	Ok(title.to_owned())
}

fn validate_summary(raw: &str) -> Result<String, DomainError> {
	let summary = raw.trim();
	if summary.chars().count() > MAX_SUMMARY_LEN {
		return Err(DomainError::Validation(format!("allocation summary must be at most {MAX_SUMMARY_LEN} characters")));
	}
	Ok(summary.to_owned())
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

/// Facts raised by the [`Allocation`] aggregate. Audit only — the relay never sees
/// them (they carry no money), so they are written to `event_log` with `relay = false`.
/// Internally tagged so the stored JSON is self-describing.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AllocationEvent {
	/// A service id became a registered product (in `Draft`).
	Registered {
		allocation_id: AllocationId,
		service: ServiceId,
		title: String,
		summary: String,
	},
	/// Presentation fields changed; state and identity did not.
	DetailsUpdated {
		allocation_id: AllocationId,
		service: ServiceId,
		title: String,
		summary: String,
	},
	/// Now accepting subscriptions.
	Opened { allocation_id: AllocationId, service: ServiceId },
	/// No longer accepting subscriptions; redemptions continue.
	Closed { allocation_id: AllocationId, service: ServiceId },
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

	fn svc() -> ServiceId {
		ServiceId::parse("trading").unwrap()
	}

	fn registered() -> Allocation {
		Allocation::register(AllocationId::new(), svc(), "EV Trading", "Systematic crypto trading").unwrap()
	}

	#[test]
	fn allocation_state_strings_are_canonical() {
		// Wire contract: these must match `evbanking_contracts::allocation::state`.
		assert_eq!(AllocationState::Draft.as_str(), "draft");
		assert_eq!(AllocationState::Open.as_str(), "open");
		assert_eq!(AllocationState::Closed.as_str(), "closed");
		for state in [AllocationState::Draft, AllocationState::Open, AllocationState::Closed] {
			assert_eq!(AllocationState::parse(state.as_str()).unwrap(), state);
		}
		assert!(AllocationState::parse("delisted").is_err());
	}

	#[test]
	fn register_starts_in_draft_and_emits_registered() {
		let mut allocation = registered();
		assert_eq!(allocation.state(), AllocationState::Draft);
		assert!(!allocation.is_listed());
		let events = allocation.drain_events();
		assert_eq!(events.len(), 1);
		assert!(matches!(events[0], AllocationEvent::Registered { .. }));
		assert!(allocation.drain_events().is_empty());
	}

	#[test]
	fn a_draft_takes_no_money_until_opened() {
		let mut allocation = registered();
		assert!(allocation.ensure_subscribable().is_err());
		allocation.open();
		assert!(allocation.ensure_subscribable().is_ok());
		assert!(allocation.is_listed());
	}

	#[test]
	fn closing_blocks_new_money_but_never_traps_an_investor() {
		let mut allocation = registered();
		allocation.open();
		allocation.close();
		assert!(allocation.ensure_subscribable().is_err());
		// The whole point of the split gate: units already minted can still be redeemed.
		assert!(allocation.ensure_redeemable().is_ok());
		assert!(!allocation.is_listed());
	}

	#[test]
	fn a_draft_is_not_redeemable_because_it_can_hold_no_units() {
		assert!(registered().ensure_redeemable().is_err());
	}

	#[test]
	fn transitions_are_idempotent_and_reopenable() {
		let mut allocation = registered();
		allocation.drain_events();
		allocation.open();
		allocation.open();
		assert_eq!(allocation.drain_events().len(), 1, "re-opening an open allocation raises nothing");
		allocation.close();
		allocation.close();
		assert_eq!(allocation.drain_events().len(), 1);
		allocation.open();
		assert_eq!(allocation.state(), AllocationState::Open);
		assert_eq!(allocation.drain_events().len(), 1, "closed → open re-opens");
	}

	#[test]
	fn details_are_trimmed_and_bounded() {
		let mut allocation = registered();
		allocation.update_details("  EV Real Estate  ", "  Income-producing property  ").unwrap();
		assert_eq!(allocation.title(), "EV Real Estate");
		assert_eq!(allocation.summary(), "Income-producing property");
		assert!(allocation.update_details("", "").is_err());
		assert!(allocation.update_details(&"x".repeat(MAX_TITLE_LEN + 1), "").is_err());
		assert!(allocation.update_details("ok", &"x".repeat(MAX_SUMMARY_LEN + 1)).is_err());
		// An empty summary is legitimate — not every product needs a one-liner.
		assert!(allocation.update_details("ok", "").is_ok());
	}

	#[test]
	fn updating_details_leaves_state_alone() {
		let mut allocation = registered();
		allocation.open();
		allocation.drain_events();
		allocation.update_details("Renamed", "").unwrap();
		assert_eq!(allocation.state(), AllocationState::Open);
		let events = allocation.drain_events();
		assert_eq!(events.len(), 1);
		assert!(matches!(events[0], AllocationEvent::DetailsUpdated { .. }));
	}

	#[test]
	fn event_round_trips_through_json() {
		let mut allocation = registered();
		let event = allocation.drain_events().pop().unwrap();
		let json = serde_json::to_string(&event).unwrap();
		let back: AllocationEvent = serde_json::from_str(&json).unwrap();
		assert!(matches!(back, AllocationEvent::Registered { .. }));
	}
}
