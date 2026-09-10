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

use crate::{balance::ServiceId, error::DomainError, money::Shares};

/// The longest accepted display title.
const MAX_TITLE_LEN: usize = 120;
/// The longest accepted catalog one-liner.
const MAX_SUMMARY_LEN: usize = 280;
/// The authorised unit supply a product is registered with: 100,000,000 units.
///
/// High enough that it never surprises an operator who did not think about it, finite so
/// that "unlimited" is never the stored answer — the registry always has a number to show
/// and to refuse against. An operator narrows it (to a thousand, say) before opening.
pub const DEFAULT_UNIT_CAP: Shares = Shares::from_base_units(100_000_000 * 1_000_000_000_000_000_000);

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

/// The catalog glyph an operator picks for a product.
///
/// A closed vocabulary rather than a free-form asset reference: the client owns the
/// artwork and maps each variant onto one of its SVGs, so a registry row can never
/// point at a picture the shipped client has no way to draw. Adding a variant means
/// shipping the client that knows it first.
///
/// Presentation only — nothing here gates money or lifecycle.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AllocationIcon {
	/// The neutral default every product lands on until an operator picks one.
	#[default]
	Fund,
	RealEstate,
	Trading,
	Yield,
	Venture,
	Treasury,
	Commodity,
	Credit,
	Index,
	Arbitrage,
}

impl AllocationIcon {
	/// The stored/wire discriminant. Keep byte-identical with
	/// `evbanking_contracts::allocation::icon` (`allocation_icon_strings_are_canonical`
	/// guards this side).
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Fund => "fund",
			Self::RealEstate => "real_estate",
			Self::Trading => "trading",
			Self::Yield => "yield",
			Self::Venture => "venture",
			Self::Treasury => "treasury",
			Self::Commodity => "commodity",
			Self::Credit => "credit",
			Self::Index => "index",
			Self::Arbitrage => "arbitrage",
		}
	}

	/// Parse the stored/wire form. An unrecognized value is an error rather than a
	/// silent fallback to [`Self::Fund`]: a client sending an icon this build does not
	/// know is a contract mismatch the operator has to see, not a product that quietly
	/// renders as something else.
	pub fn parse(raw: &str) -> Result<Self, DomainError> {
		match raw {
			"fund" => Ok(Self::Fund),
			"real_estate" => Ok(Self::RealEstate),
			"trading" => Ok(Self::Trading),
			"yield" => Ok(Self::Yield),
			"venture" => Ok(Self::Venture),
			"treasury" => Ok(Self::Treasury),
			"commodity" => Ok(Self::Commodity),
			"credit" => Ok(Self::Credit),
			"index" => Ok(Self::Index),
			"arbitrage" => Ok(Self::Arbitrage),
			other => Err(DomainError::Validation(format!("unknown allocation icon: {other}"))),
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
	unit_cap: Shares,
	icon: AllocationIcon,
	pending: Vec<AllocationEvent>,
}

impl Allocation {
	/// Register `service` as an investable product, in [`AllocationState::Draft`] and at
	/// [`DEFAULT_UNIT_CAP`] — a registration never opens for business in the same step,
	/// so listing, sizing and funding stay separate operator decisions. Raises
	/// `Registered`.
	pub fn register(id: AllocationId, service: ServiceId, title: &str, summary: &str, icon: AllocationIcon) -> Result<Self, DomainError> {
		let title = validate_title(title)?;
		let summary = validate_summary(summary)?;
		let mut allocation = Self {
			id,
			service: service.clone(),
			title: title.clone(),
			summary: summary.clone(),
			state: AllocationState::Draft,
			unit_cap: DEFAULT_UNIT_CAP,
			icon,
			pending: Vec::new(),
		};
		allocation.pending.push(AllocationEvent::Registered {
			allocation_id: id,
			service,
			title,
			summary,
			unit_cap: DEFAULT_UNIT_CAP,
			icon,
		});
		Ok(allocation)
	}

	/// Reconstitute from the store. Raises no events.
	pub fn rehydrate(id: AllocationId, service: ServiceId, title: String, summary: String, state: AllocationState, unit_cap: Shares, icon: AllocationIcon) -> Self {
		Self {
			id,
			service,
			title,
			summary,
			state,
			unit_cap,
			icon,
			pending: Vec::new(),
		}
	}

	/// Replace the presentation fields — title, summary and icon. Identity and state are
	/// untouched, so an operator may fix a title on a live product without disturbing
	/// money flow. The icon rides here rather than in a command of its own because it is
	/// presentation like the other two and gates nothing. Raises `DetailsUpdated`.
	pub fn update_details(&mut self, title: &str, summary: &str, icon: AllocationIcon) -> Result<(), DomainError> {
		self.title = validate_title(title)?;
		self.summary = validate_summary(summary)?;
		self.icon = icon;
		self.pending.push(AllocationEvent::DetailsUpdated {
			allocation_id: self.id,
			service: self.service.clone(),
			title: self.title.clone(),
			summary: self.summary.clone(),
			icon,
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

	/// Resize the authorised unit supply. Idempotent: setting the cap it already has
	/// raises nothing. Raises `CapUpdated`.
	///
	/// A cap **below** the units already issued is deliberately legal — it is how an
	/// operator stops issuance on a product that has run further than intended. It means
	/// "no more units", not "some units are now invalid": nothing already minted is
	/// touched, and [`Self::ensure_redeemable`] is not consulted here, so holders still
	/// get out. Only zero is refused, because a zero cap is indistinguishable from an
	/// unset one and would silently close a live product to new money without saying so
	/// in its state.
	pub fn set_unit_cap(&mut self, unit_cap: Shares) -> Result<(), DomainError> {
		if unit_cap.is_zero() {
			return Err(DomainError::Validation(
				"allocation unit cap must be greater than zero — close the allocation to stop new money".into(),
			));
		}
		if unit_cap == self.unit_cap {
			return Ok(());
		}
		self.unit_cap = unit_cap;
		self.pending.push(AllocationEvent::CapUpdated {
			allocation_id: self.id,
			service: self.service.clone(),
			unit_cap,
		});
		Ok(())
	}

	/// The supply gate the subscribe path runs once it knows what it would mint:
	/// `issued + minting` must stay within the cap.
	///
	/// `issued` is what the caller reads from the ledger; this aggregate does no I/O, so
	/// it takes the figure rather than fetching it. Overflow is refused rather than
	/// wrapped — an addition that cannot be represented cannot be shown to be under a cap.
	pub fn ensure_capacity(&self, issued: Shares, minting: Shares) -> Result<(), DomainError> {
		let after = issued
			.checked_add(minting)
			.ok_or_else(|| DomainError::Validation(format!("allocation '{}' cannot mint that many units", self.service)))?;
		if after > self.unit_cap {
			let remaining = self.unit_cap.checked_sub(issued).unwrap_or(Shares::ZERO);
			return Err(DomainError::Validation(format!(
				"allocation '{}' has {} units left of its {} unit cap — this subscription would mint {}",
				self.service,
				remaining.to_decimal_string(),
				self.unit_cap.to_decimal_string(),
				minting.to_decimal_string(),
			)));
		}
		Ok(())
	}

	/// Units still issuable against the cap, given what the ledger says is already out.
	/// Saturating: a cap narrowed below the issued supply reports zero headroom, not a
	/// negative one.
	pub fn remaining_capacity(&self, issued: Shares) -> Shares {
		self.unit_cap.checked_sub(issued).unwrap_or(Shares::ZERO)
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

	pub fn unit_cap(&self) -> Shares {
		self.unit_cap
	}

	pub fn icon(&self) -> AllocationIcon {
		self.icon
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
	/// A service id became a registered product (in `Draft`, at the default unit cap).
	Registered {
		allocation_id: AllocationId,
		service: ServiceId,
		title: String,
		summary: String,
		unit_cap: Shares,
		/// `default` so a row written before icons existed still deserializes — those
		/// products were all rendered as the letter avatar this default replaces, and
		/// the column backfills to the same value.
		#[serde(default)]
		icon: AllocationIcon,
	},
	/// Presentation fields changed; state and identity did not.
	DetailsUpdated {
		allocation_id: AllocationId,
		service: ServiceId,
		title: String,
		summary: String,
		/// `default` for the same reason as on `Registered` — it keeps pre-icon rows
		/// readable.
		#[serde(default)]
		icon: AllocationIcon,
	},
	/// The authorised unit supply was resized. Its own fact rather than a field on
	/// `DetailsUpdated`: this one gates money, so "who changed the cap, and when" has to
	/// be answerable without diffing every title edit.
	CapUpdated { allocation_id: AllocationId, service: ServiceId, unit_cap: Shares },
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
		Allocation::register(AllocationId::new(), svc(), "EV Trading", "Systematic crypto trading", AllocationIcon::Trading).unwrap()
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
	fn allocation_icon_strings_are_canonical() {
		// Wire contract: these must match `evbanking_contracts::allocation::icon`.
		let expected = [
			(AllocationIcon::Fund, "fund"),
			(AllocationIcon::RealEstate, "real_estate"),
			(AllocationIcon::Trading, "trading"),
			(AllocationIcon::Yield, "yield"),
			(AllocationIcon::Venture, "venture"),
			(AllocationIcon::Treasury, "treasury"),
			(AllocationIcon::Commodity, "commodity"),
			(AllocationIcon::Credit, "credit"),
			(AllocationIcon::Index, "index"),
			(AllocationIcon::Arbitrage, "arbitrage"),
		];
		for (icon, wire) in expected {
			assert_eq!(icon.as_str(), wire);
			assert_eq!(AllocationIcon::parse(wire).unwrap(), icon);
		}
	}

	#[test]
	fn an_unknown_icon_is_refused_rather_than_defaulted() {
		// A client sending an icon this build cannot draw is a contract mismatch, not a
		// product that quietly renders as something else.
		assert!(AllocationIcon::parse("rocket").is_err());
		assert!(AllocationIcon::parse("").is_err());
		assert!(AllocationIcon::parse("Fund").is_err(), "the wire form is lowercase snake_case");
		assert!(AllocationIcon::parse("realEstate").is_err());
	}

	#[test]
	fn the_default_icon_is_fund() {
		// What a product lands on when the operator picks nothing, and what the column
		// and the pre-icon event payloads backfill to.
		assert_eq!(AllocationIcon::default(), AllocationIcon::Fund);
		assert_eq!(AllocationIcon::default().as_str(), "fund");
	}

	#[test]
	fn the_icon_serializes_to_its_wire_string() {
		// The stored event payload spells the icon exactly as `as_str` does — the guard
		// against the serde rename and the manual table drifting apart.
		for icon in [AllocationIcon::RealEstate, AllocationIcon::Arbitrage, AllocationIcon::Fund] {
			let json = serde_json::to_string(&icon).unwrap();
			assert_eq!(json, format!("\"{}\"", icon.as_str()));
			assert_eq!(serde_json::from_str::<AllocationIcon>(&json).unwrap(), icon);
		}
	}

	#[test]
	fn a_pre_icon_event_payload_still_deserializes() {
		// `event_log` rows written before this field existed carry no `icon` key. Without
		// the serde default they would stop deserializing forever, and an audit read of
		// the registry's history would fail on every historical row.
		let mut allocation = registered();
		let registered_json = serde_json::to_string(&allocation.drain_events().pop().unwrap()).unwrap();
		allocation.update_details("Renamed", "", AllocationIcon::Venture).unwrap();
		let updated_json = serde_json::to_string(&allocation.drain_events().pop().unwrap()).unwrap();
		for json in [registered_json, updated_json] {
			let legacy = json.replace(r#","icon":"trading""#, "").replace(r#","icon":"venture""#, "");
			assert!(!legacy.contains("icon"), "the legacy payload must actually lack the key: {legacy}");
			let back: AllocationEvent = serde_json::from_str(&legacy).unwrap();
			let icon = match back {
				AllocationEvent::Registered { icon, .. } | AllocationEvent::DetailsUpdated { icon, .. } => icon,
				other => panic!("unexpected event: {other:?}"),
			};
			assert_eq!(icon, AllocationIcon::Fund, "a pre-icon row reads back as the default");
		}
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
	fn a_registration_starts_at_the_default_cap() {
		let allocation = registered();
		assert_eq!(allocation.unit_cap(), DEFAULT_UNIT_CAP);
		assert_eq!(DEFAULT_UNIT_CAP.to_decimal_string(), "100000000");
	}

	#[test]
	fn capacity_admits_up_to_the_cap_and_refuses_the_unit_past_it() {
		let mut allocation = registered();
		allocation.set_unit_cap(Shares::parse_decimal("1000").unwrap()).unwrap();
		let issued = Shares::parse_decimal("999").unwrap();
		// Landing exactly on the cap is allowed; it is the cap, not a ceiling to stay under.
		assert!(allocation.ensure_capacity(issued, Shares::parse_decimal("1").unwrap()).is_ok());
		// One base unit more is not. The message names the headroom the caller has left.
		let err = allocation.ensure_capacity(issued, Shares::from_base_units(1_000_000_000_000_000_000 + 1)).unwrap_err();
		assert!(matches!(err, DomainError::Validation(ref m) if m.contains("1 units left")), "{err:?}");
	}

	#[test]
	fn a_cap_below_the_issued_supply_stops_issuance_without_trapping_anyone() {
		let mut allocation = registered();
		allocation.open();
		allocation.set_unit_cap(Shares::parse_decimal("100").unwrap()).unwrap();
		let issued = Shares::parse_decimal("500").unwrap();
		assert_eq!(allocation.remaining_capacity(issued), Shares::ZERO);
		assert!(allocation.ensure_capacity(issued, Shares::from_base_units(1)).is_err());
		// Narrowing the cap is a supply decision, not a lifecycle one — holders still exit.
		assert!(allocation.ensure_redeemable().is_ok());
	}

	#[test]
	fn setting_the_cap_is_idempotent_validated_and_audited() {
		let mut allocation = registered();
		allocation.drain_events();
		// Zero would read as "unset" while silently refusing every subscription.
		assert!(allocation.set_unit_cap(Shares::ZERO).is_err());
		allocation.set_unit_cap(Shares::parse_decimal("1000").unwrap()).unwrap();
		allocation.set_unit_cap(Shares::parse_decimal("1000").unwrap()).unwrap();
		let events = allocation.drain_events();
		assert_eq!(events.len(), 1, "re-setting the same cap raises nothing");
		assert!(matches!(events[0], AllocationEvent::CapUpdated { .. }));
		assert_eq!(allocation.unit_cap(), Shares::parse_decimal("1000").unwrap());
	}

	#[test]
	fn capacity_refuses_a_mint_it_cannot_even_add_up() {
		let allocation = registered();
		// `issued + minting` overflowing u128 is refused, never wrapped into "under cap".
		assert!(allocation.ensure_capacity(Shares::from_base_units(u128::MAX), Shares::from_base_units(1)).is_err());
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
		allocation
			.update_details("  EV Real Estate  ", "  Income-producing property  ", AllocationIcon::RealEstate)
			.unwrap();
		assert_eq!(allocation.title(), "EV Real Estate");
		assert_eq!(allocation.summary(), "Income-producing property");
		assert_eq!(allocation.icon(), AllocationIcon::RealEstate);
		assert!(allocation.update_details("", "", AllocationIcon::Fund).is_err());
		assert!(allocation.update_details(&"x".repeat(MAX_TITLE_LEN + 1), "", AllocationIcon::Fund).is_err());
		assert!(allocation.update_details("ok", &"x".repeat(MAX_SUMMARY_LEN + 1), AllocationIcon::Fund).is_err());
		// An empty summary is legitimate — not every product needs a one-liner.
		assert!(allocation.update_details("ok", "", AllocationIcon::Fund).is_ok());
	}

	#[test]
	fn updating_details_leaves_state_alone() {
		let mut allocation = registered();
		allocation.open();
		allocation.drain_events();
		allocation.update_details("Renamed", "", AllocationIcon::Trading).unwrap();
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
