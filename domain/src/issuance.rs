//! `issuance` bounded context — an operator issuing fund units **in kind**.
//!
//! A [`Subscription`](crate::subscriptions::Subscription) is the only way units came
//! into existence until now: cash leaves a user's claim, units arrive in their holding.
//! That leaves no honest way to register a product against an asset the company already
//! owns — a service whose valuation is known, whose supply is fixed, and where the
//! company and a named investor hold their shares because they *own the asset*, not
//! because they wired cash into a fund claim. A [`UnitIssuance`] is that door: a mint
//! with **no cash leg**, to a [`UnitHolder`] that may be a user or the company itself,
//! at a cost basis the operator states (or `units × NAV` when they do not).
//!
//! The relay posts one leg, `Dr <holder shares> / Cr SharesOutstanding`, so the supply
//! invariant `SharesOutstanding == Σ UserShares + FeeShares + CompanyShares` holds by
//! construction and the issued units count against the allocation's cap exactly as a
//! subscription's do. The cash plane is untouched: no claim moves, and the global
//! `sum(custody) == sum(claims)` is not involved at all.
//!
//! The same record has a second [`IssuanceSource`]: units handed to a user **out of the
//! company's stake** ([`UnitIssuance::transfer_company_stake`]). The company cannot deal
//! on the book (it has no user, so no orders and no escrow), and minting the user a
//! second copy of what the company holds would inflate supply and break the cap table —
//! so the relay moves the units *between holders*, `Dr UserShares / Cr CompanyShares`,
//! and `SharesOutstanding` stays where it is. It is an issuance from the recipient's
//! side (units they did not pay cash for, at a stated basis, blended into their
//! high-water mark at the mark) and a transfer from the ledger's, which is why it is a
//! variant of this aggregate and not a second one.
//!
//! Idempotent by an operator-supplied [`IdempotencyKey`], unique per service: an admin
//! console that retries a timed-out request must land one mint, never two. The
//! aggregate is an immutable record with one relay-driven transition, `Queued` →
//! `Applied`, stamped once the mint has posted.
//!
//! Pure and wasm-safe (mirrors [`Subscription`](crate::subscriptions::Subscription)):
//! ids are minted by the application layer, no clock, no I/O. NAV is supplied by the
//! caller, never derived here.

use ev::architecture::{AggregateRoot, DomainEvent, EmitsEvents, Entity, Id};
use serde::{Deserialize, Serialize};

use crate::{
	balance::{LedgerAccountKey, ServiceId},
	error::DomainError,
	money::{Nav, Shares, Usdt},
	users::UserId,
};

/// A unique issuance id (UUID). Minted by the application layer.
pub type UnitIssuanceId = Id<UnitIssuanceTag>;
/// Phantom tag making [`UnitIssuanceId`] a distinct, incompatible identity type.
pub struct UnitIssuanceTag;

/// The longest accepted idempotency key.
pub const MAX_IDEMPOTENCY_KEY_LEN: usize = 64;

/// Who receives issued units. Tagged like [`Party`](crate::balance::Party) so the
/// stored event payload is self-describing.
///
/// The company is a holder in its own right rather than a user with a well-known id:
/// it has no `users` row, no cost-basis projection and no P&L to report, and giving it
/// a synthetic user would drag every investor-facing read (positions, fees, the
/// activity feed) into special-casing one UUID.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum UnitHolder {
	/// An investor: units land in their `UserShares` and their `fund_positions`
	/// projection gains the issuance's cost basis.
	User(UserId),
	/// The company's own stake: units land in `CompanyShares`. No projection.
	Company,
}

impl UnitHolder {
	/// The discriminator stored in the `holder_kind` column. Keep byte-identical with
	/// `evbanking_contracts::allocation::holder` (`unit_holder_strings_are_canonical`
	/// guards this side).
	pub fn kind_str(&self) -> &'static str {
		match self {
			Self::User(_) => "user",
			Self::Company => "company",
		}
	}

	/// The identity stored in the `holder_id` column (`None` for the company).
	pub fn user_id(&self) -> Option<UserId> {
		match self {
			Self::User(id) => Some(*id),
			Self::Company => None,
		}
	}

	/// Reconstruct from the `(kind, id)` column pair (persistence adapter).
	pub fn from_parts(kind: &str, id: Option<UserId>) -> Result<Self, DomainError> {
		match (kind, id) {
			("user", Some(user)) => Ok(Self::User(user)),
			("company", None) => Ok(Self::Company),
			_ => Err(DomainError::Validation(format!("invalid unit holder: {kind}"))),
		}
	}

	/// The debit-normal Share-ledger account the issued units are minted into.
	pub fn shares_key(&self, service: &ServiceId) -> LedgerAccountKey {
		match self {
			Self::User(user) => LedgerAccountKey::UserShares(service.clone(), *user),
			Self::Company => LedgerAccountKey::CompanyShares(service.clone()),
		}
	}
}

/// Where an issuance stands: recorded (`Queued`) or minted on the ledger (`Applied`).
/// The relay is the only writer of the second state, after the mint posts.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IssuanceState {
	Queued,
	Applied,
}

impl IssuanceState {
	/// The stored/wire discriminant. Keep byte-identical with
	/// `evbanking_contracts::allocation::issuance_state`
	/// (`issuance_state_strings_are_canonical` guards this side).
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Queued => "queued",
			Self::Applied => "applied",
		}
	}

	/// Parse the stored/wire form. An unrecognized value is an error rather than a
	/// silent default, so a corrupt row never reads as minted when it was not.
	pub fn parse(raw: &str) -> Result<Self, DomainError> {
		match raw {
			"queued" => Ok(Self::Queued),
			"applied" => Ok(Self::Applied),
			other => Err(DomainError::Validation(format!("unknown issuance state: {other}"))),
		}
	}
}

/// Where an issuance's units come from. `Mint` grows supply; `Company` moves units the
/// company already holds and leaves supply alone — the ledger leg differs, the record
/// and the recipient's projection do not.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IssuanceSource {
	/// Minted in kind: `Dr <holder shares> / Cr SharesOutstanding`.
	#[default]
	Mint,
	/// Out of the company's stake: `Dr UserShares / Cr CompanyShares`. The holder is
	/// always a user — the company handing units to itself is not a request.
	Company,
}

impl IssuanceSource {
	/// The stored/wire discriminant. Keep byte-identical with
	/// `evbanking_contracts::allocation::issuance_source`
	/// (`issuance_source_strings_are_canonical` guards this side).
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Mint => "mint",
			Self::Company => "company",
		}
	}

	/// Parse the stored/wire form. An unrecognized value is an error rather than a
	/// silent default, so a corrupt row never reads as a mint when it moved the
	/// company's units.
	pub fn parse(raw: &str) -> Result<Self, DomainError> {
		match raw {
			"mint" => Ok(Self::Mint),
			"company" => Ok(Self::Company),
			other => Err(DomainError::Validation(format!("unknown issuance source: {other}"))),
		}
	}
}

/// The operator's retry key for one issuance, unique per service. Trimmed, 1..=64
/// chars: long enough for a UUID, short enough to index.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
	pub fn parse(raw: &str) -> Result<Self, DomainError> {
		let value = raw.trim();
		if value.is_empty() || value.chars().count() > MAX_IDEMPOTENCY_KEY_LEN {
			return Err(DomainError::Validation(format!("idempotency key must be 1..{MAX_IDEMPOTENCY_KEY_LEN} chars")));
		}
		Ok(Self(value.to_owned()))
	}

	pub fn as_str(&self) -> &str {
		&self.0
	}
}

/// The stored shape of a [`UnitIssuance`], as the persistence adapter reads it back —
/// the input to [`UnitIssuance::rehydrate`].
pub struct UnitIssuanceSnapshot {
	pub id: UnitIssuanceId,
	pub service: ServiceId,
	pub holder: UnitHolder,
	pub source: IssuanceSource,
	pub units: Shares,
	pub nav: Nav,
	pub cost_basis: Usdt,
	pub idempotency_key: IdempotencyKey,
	pub state: IssuanceState,
}

/// The issuance aggregate — one in-kind mint, or one hand-over out of the company's
/// stake. Construct via [`UnitIssuance::issue`] / [`UnitIssuance::transfer_company_stake`]
/// (both raise [`IssuanceEvent::Issued`]) or [`UnitIssuance::rehydrate`] (load from the
/// store, no events).
#[derive(Clone, Debug)]
pub struct UnitIssuance {
	id: UnitIssuanceId,
	service: ServiceId,
	holder: UnitHolder,
	source: IssuanceSource,
	units: Shares,
	nav: Nav,
	cost_basis: Usdt,
	idempotency_key: IdempotencyKey,
	state: IssuanceState,
	pending: Vec<IssuanceEvent>,
}

impl UnitIssuance {
	/// Issue `units` of `service` to `holder` at `nav`. `cost_basis` is what the holder
	/// is deemed to have paid; `None` prices it as `units × nav`, the same figure a
	/// subscription for those units would have cost at this mark. An explicit `Some`
	/// may be anything, zero included — the company's stake in an asset it already
	/// owned cost it no cash. Rejects zero `units`. Raises `Issued`.
	pub fn issue(
		id: UnitIssuanceId,
		service: ServiceId,
		holder: UnitHolder,
		units: Shares,
		nav: Nav,
		cost_basis: Option<Usdt>,
		idempotency_key: IdempotencyKey,
	) -> Result<Self, DomainError> {
		Self::record(id, service, holder, IssuanceSource::Mint, units, nav, cost_basis, idempotency_key)
	}

	/// Hand `units` of the company's stake in `service` to `user` at `nav`. Same
	/// record, same basis rule and same `Issued` event as [`Self::issue`], but the relay
	/// posts `Dr UserShares / Cr CompanyShares` — supply does not move. Whether the
	/// company actually holds `units` is a ledger fact the use case reads first
	/// (Read-First) and TigerBeetle's non-negative flag on `CompanyShares` backstops; the
	/// aggregate cannot know it. Rejects zero `units`.
	pub fn transfer_company_stake(
		id: UnitIssuanceId,
		service: ServiceId,
		user: UserId,
		units: Shares,
		nav: Nav,
		cost_basis: Option<Usdt>,
		idempotency_key: IdempotencyKey,
	) -> Result<Self, DomainError> {
		Self::record(id, service, UnitHolder::User(user), IssuanceSource::Company, units, nav, cost_basis, idempotency_key)
	}

	// One private constructor behind two typed doors; a builder would only rename the
	// same eight facts.
	#[allow(clippy::too_many_arguments)]
	fn record(
		id: UnitIssuanceId,
		service: ServiceId,
		holder: UnitHolder,
		source: IssuanceSource,
		units: Shares,
		nav: Nav,
		cost_basis: Option<Usdt>,
		idempotency_key: IdempotencyKey,
	) -> Result<Self, DomainError> {
		if units.is_zero() {
			return Err(DomainError::Validation("issued units must be positive".into()));
		}
		let cost_basis = match cost_basis {
			Some(basis) => basis,
			None => nav.value(units)?,
		};
		let mut issuance = Self {
			id,
			service: service.clone(),
			holder,
			source,
			units,
			nav,
			cost_basis,
			idempotency_key,
			state: IssuanceState::Queued,
			pending: Vec::new(),
		};
		issuance.pending.push(IssuanceEvent::Issued {
			issuance_id: id,
			service,
			holder,
			source,
			units,
			nav,
			cost_basis,
		});
		Ok(issuance)
	}

	/// Reconstitute from the store. Raises no events.
	pub fn rehydrate(snapshot: UnitIssuanceSnapshot) -> Self {
		Self {
			id: snapshot.id,
			service: snapshot.service,
			holder: snapshot.holder,
			source: snapshot.source,
			units: snapshot.units,
			nav: snapshot.nav,
			cost_basis: snapshot.cost_basis,
			idempotency_key: snapshot.idempotency_key,
			state: snapshot.state,
			pending: Vec::new(),
		}
	}

	/// Whether a retry under the same key is asking for the same thing. Holder, source
	/// and units are the identity of the request; NAV and the defaulted cost basis are
	/// what the hub computed for it, and a retry a minute later must not be refused
	/// because the mark moved in between.
	pub fn matches_request(&self, holder: UnitHolder, source: IssuanceSource, units: Shares) -> bool {
		self.holder == holder && self.source == source && self.units == units
	}

	pub fn id(&self) -> UnitIssuanceId {
		self.id
	}

	pub fn service(&self) -> &ServiceId {
		&self.service
	}

	pub fn holder(&self) -> UnitHolder {
		self.holder
	}

	pub fn source(&self) -> IssuanceSource {
		self.source
	}

	pub fn units(&self) -> Shares {
		self.units
	}

	pub fn nav(&self) -> Nav {
		self.nav
	}

	pub fn cost_basis(&self) -> Usdt {
		self.cost_basis
	}

	pub fn idempotency_key(&self) -> &IdempotencyKey {
		&self.idempotency_key
	}

	pub fn state(&self) -> IssuanceState {
		self.state
	}
}

impl Entity for UnitIssuance {
	type Id = UnitIssuanceId;

	fn id(&self) -> UnitIssuanceId {
		self.id
	}
}

impl AggregateRoot for UnitIssuance {
	const NAME: &'static str = "unit_issuance";
}

/// Facts raised by the [`UnitIssuance`] aggregate. `Issued` carries everything the
/// relay needs to post the leg and, for a user holder, project the cost basis — no
/// extra read. Internally tagged so the stored JSON is self-describing.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IssuanceEvent {
	/// Units handed to a holder with no cash leg (relay, by `source`: `Mint` → `Dr
	/// <holder shares> / Cr SharesOutstanding`, `Company` → `Dr UserShares / Cr
	/// CompanyShares`, for `units`; then, for a user holder, `fund_positions.cost_basis
	/// += cost_basis`).
	Issued {
		issuance_id: UnitIssuanceId,
		service: ServiceId,
		holder: UnitHolder,
		/// Defaulted on read because payloads written before the field existed — the
		/// permanent `event_log`, and an outbox row undrained across a deploy — were all
		/// mints, and a stored fact must not become unreadable when the vocabulary grows.
		#[serde(default)]
		source: IssuanceSource,
		units: Shares,
		nav: Nav,
		cost_basis: Usdt,
	},
}

impl DomainEvent for IssuanceEvent {
	const KIND: &'static str = "issuances";
}

impl EmitsEvents for UnitIssuance {
	type Event = IssuanceEvent;

	fn drain_events(&mut self) -> Vec<IssuanceEvent> {
		core::mem::take(&mut self.pending)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn svc() -> ServiceId {
		ServiceId::parse("service_arb").unwrap()
	}

	fn key() -> IdempotencyKey {
		IdempotencyKey::parse("issue-1").unwrap()
	}

	#[test]
	fn unit_holder_strings_are_canonical() {
		// Wire contract: these must match `evbanking_contracts::allocation::holder`.
		let user = UserId::new();
		assert_eq!(UnitHolder::User(user).kind_str(), "user");
		assert_eq!(UnitHolder::Company.kind_str(), "company");
		for holder in [UnitHolder::User(user), UnitHolder::Company] {
			assert_eq!(UnitHolder::from_parts(holder.kind_str(), holder.user_id()).unwrap(), holder);
		}
		// A user without an id, a company with one, and an unknown kind are all corrupt rows.
		assert!(UnitHolder::from_parts("user", None).is_err());
		assert!(UnitHolder::from_parts("company", Some(user)).is_err());
		assert!(UnitHolder::from_parts("fund", None).is_err());
	}

	#[test]
	fn issuance_state_strings_are_canonical() {
		// Wire contract: these must match `evbanking_contracts::allocation::issuance_state`.
		assert_eq!(IssuanceState::Queued.as_str(), "queued");
		assert_eq!(IssuanceState::Applied.as_str(), "applied");
		for state in [IssuanceState::Queued, IssuanceState::Applied] {
			assert_eq!(IssuanceState::parse(state.as_str()).unwrap(), state);
			assert_eq!(serde_json::to_string(&state).unwrap(), format!("\"{}\"", state.as_str()));
		}
		assert!(IssuanceState::parse("minted").is_err());
	}

	#[test]
	fn issuance_source_strings_are_canonical() {
		// Wire contract: these must match `evbanking_contracts::allocation::issuance_source`.
		assert_eq!(IssuanceSource::Mint.as_str(), "mint");
		assert_eq!(IssuanceSource::Company.as_str(), "company");
		for source in [IssuanceSource::Mint, IssuanceSource::Company] {
			assert_eq!(IssuanceSource::parse(source.as_str()).unwrap(), source);
			assert_eq!(serde_json::to_string(&source).unwrap(), format!("\"{}\"", source.as_str()));
		}
		assert!(IssuanceSource::parse("transfer").is_err());
	}

	#[test]
	fn each_holder_mints_into_its_own_share_account() {
		let user = UserId::new();
		assert_eq!(UnitHolder::User(user).shares_key(&svc()), LedgerAccountKey::UserShares(svc(), user));
		assert_eq!(UnitHolder::Company.shares_key(&svc()), LedgerAccountKey::CompanyShares(svc()));
	}

	#[test]
	fn the_idempotency_key_is_trimmed_and_bounded() {
		assert_eq!(IdempotencyKey::parse("  retry-7 ").unwrap().as_str(), "retry-7");
		assert!(IdempotencyKey::parse("").is_err());
		assert!(IdempotencyKey::parse("   ").is_err());
		assert!(IdempotencyKey::parse(&"k".repeat(MAX_IDEMPOTENCY_KEY_LEN)).is_ok());
		assert!(IdempotencyKey::parse(&"k".repeat(MAX_IDEMPOTENCY_KEY_LEN + 1)).is_err());
	}

	#[test]
	fn the_cost_basis_defaults_to_units_times_nav_and_may_be_overridden() {
		// 800 units at NAV 1.25 → 1000 USDT when the operator states nothing.
		let defaulted = UnitIssuance::issue(
			UnitIssuanceId::new(),
			svc(),
			UnitHolder::Company,
			Shares::parse_decimal("800").unwrap(),
			Nav::parse_decimal("1.25").unwrap(),
			None,
			key(),
		)
		.unwrap();
		assert_eq!(defaulted.cost_basis(), Usdt::parse_decimal("1000").unwrap());
		// An explicit basis is taken as given — zero included, for an asset that cost no cash.
		let explicit = UnitIssuance::issue(
			UnitIssuanceId::new(),
			svc(),
			UnitHolder::Company,
			Shares::parse_decimal("800").unwrap(),
			Nav::SEED,
			Some(Usdt::ZERO),
			key(),
		)
		.unwrap();
		assert_eq!(explicit.cost_basis(), Usdt::ZERO);
	}

	#[test]
	fn issue_starts_queued_and_emits_issued() {
		let user = UserId::new();
		let mut issuance = UnitIssuance::issue(
			UnitIssuanceId::new(),
			svc(),
			UnitHolder::User(user),
			Shares::parse_decimal("200").unwrap(),
			Nav::SEED,
			None,
			key(),
		)
		.unwrap();
		assert_eq!(issuance.state(), IssuanceState::Queued);
		let events = issuance.drain_events();
		assert_eq!(events.len(), 1);
		assert!(matches!(events[0], IssuanceEvent::Issued { holder: UnitHolder::User(u), .. } if u == user));
		assert!(issuance.drain_events().is_empty());
	}

	#[test]
	fn zero_units_are_rejected() {
		assert!(UnitIssuance::issue(UnitIssuanceId::new(), svc(), UnitHolder::Company, Shares::ZERO, Nav::SEED, None, key()).is_err());
		assert!(UnitIssuance::transfer_company_stake(UnitIssuanceId::new(), svc(), UserId::new(), Shares::ZERO, Nav::SEED, None, key()).is_err());
	}

	#[test]
	fn a_company_stake_transfer_is_an_issuance_to_the_user_from_the_company() {
		let user = UserId::new();
		let mut transfer = UnitIssuance::transfer_company_stake(
			UnitIssuanceId::new(),
			svc(),
			user,
			Shares::parse_decimal("13000").unwrap(),
			Nav::parse_decimal("1.25").unwrap(),
			None,
			key(),
		)
		.unwrap();
		assert_eq!(transfer.holder(), UnitHolder::User(user), "the recipient is the holder of record");
		assert_eq!(transfer.source(), IssuanceSource::Company);
		assert_eq!(transfer.cost_basis(), Usdt::parse_decimal("16250").unwrap(), "defaults to units × NAV like a mint");
		assert_eq!(transfer.state(), IssuanceState::Queued);
		let events = transfer.drain_events();
		assert!(matches!(
			events.as_slice(),
			[IssuanceEvent::Issued {
				source: IssuanceSource::Company,
				holder: UnitHolder::User(u),
				..
			}] if *u == user
		));
		// A mint is still a mint.
		let mut mint = UnitIssuance::issue(UnitIssuanceId::new(), svc(), UnitHolder::User(user), Shares::parse_decimal("1").unwrap(), Nav::SEED, None, key()).unwrap();
		assert_eq!(mint.source(), IssuanceSource::Mint);
		assert!(matches!(mint.drain_events().as_slice(), [IssuanceEvent::Issued { source: IssuanceSource::Mint, .. }]));
	}

	#[test]
	fn a_retry_matches_on_holder_source_and_units_only() {
		let user = UserId::new();
		let units = Shares::parse_decimal("200").unwrap();
		let issuance = UnitIssuance::issue(UnitIssuanceId::new(), svc(), UnitHolder::User(user), units, Nav::SEED, None, key()).unwrap();
		assert!(issuance.matches_request(UnitHolder::User(user), IssuanceSource::Mint, units));
		assert!(!issuance.matches_request(UnitHolder::Company, IssuanceSource::Mint, units));
		assert!(!issuance.matches_request(UnitHolder::User(user), IssuanceSource::Mint, Shares::parse_decimal("201").unwrap()));
		// The same key naming a mint and then a hand-over of the company's stake is a
		// different request, not a retry: one grows supply, the other does not.
		assert!(!issuance.matches_request(UnitHolder::User(user), IssuanceSource::Company, units));
	}

	#[test]
	fn an_event_written_before_the_source_existed_reads_as_a_mint() {
		// The permanent event log holds `Issued` payloads with no `source`; every one of
		// them was a mint, and a stored fact must stay readable as the vocabulary grows.
		let json = format!(
			r#"{{"type":"issued","issuance_id":"{}","service":"service_arb","holder":{{"kind":"company"}},"units":"10","nav":"1","cost_basis":"10"}}"#,
			UnitIssuanceId::new()
		);
		let IssuanceEvent::Issued { source, .. } = serde_json::from_str(&json).unwrap();
		assert_eq!(source, IssuanceSource::Mint);
	}

	#[test]
	fn event_round_trips_through_json_for_both_holders() {
		for holder in [UnitHolder::User(UserId::new()), UnitHolder::Company] {
			let mut issuance = UnitIssuance::issue(UnitIssuanceId::new(), svc(), holder, Shares::parse_decimal("10").unwrap(), Nav::SEED, None, key()).unwrap();
			let event = issuance.drain_events().pop().unwrap();
			let json = serde_json::to_string(&event).unwrap();
			let back: IssuanceEvent = serde_json::from_str(&json).unwrap();
			let IssuanceEvent::Issued { holder: back_holder, .. } = back;
			assert_eq!(back_holder, holder);
		}
		// The company holder is self-describing with no id attached.
		assert_eq!(serde_json::to_string(&UnitHolder::Company).unwrap(), r#"{"kind":"company"}"#);
	}
}
