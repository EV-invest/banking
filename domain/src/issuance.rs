//! `issuance` bounded context — an operator issuing fund units **in kind**.
//!
//! A [`Subscription`](crate::subscriptions::Subscription) is the only way units came
//! into existence until now: cash leaves a user's claim, units arrive in their holding.
//! That leaves no honest way to register a product against an asset the holders already
//! own — a service whose valuation is known, whose supply is fixed, and where a named
//! investor holds their share because they *own the asset*, not because they wired cash
//! into a fund claim. A [`UnitIssuance`] is that door: a mint with **no cash leg**, to a
//! [`UnitHolder`] that is a user or a reserved allocation, at a cost basis the operator
//! states (or `units × NAV` when they do not).
//!
//! The relay posts one leg, `Dr <holder shares> / Cr SharesOutstanding`, so the supply
//! invariant `SharesOutstanding == Σ UserShares + Σ BookShares + Σ allocation-held shares`
//! holds by construction and the issued units count against the allocation's cap exactly
//! as a subscription's do. The cash plane is untouched: no claim moves, and the global
//! `sum(custody) == sum(claims)` is not involved at all.
//!
//! **Who may hold.** A user, always. An allocation only when it is one the platform
//! reserves for itself ([`ServiceId::is_reserved`] — `fee`, `fund`) and the product is
//! not: the holder graph is strictly *reserved → product*, so a product can never hold
//! itself or another product, and a reserved allocation can never hold another reserved
//! one. Every unit therefore bottoms out at a person within two hops — a product's fee
//! class is held by `fee`, and `fee` is held by people (issue #245). In phase 1 the
//! `fee` allocation is the only allocation holder: its holding of a product is the
//! product's fee class, the physical `FeeShares` account. The `fund` allocation holds no
//! product units yet — its capital is cash on its own claim — so an issuance naming it is
//! refused rather than aliased onto an account that does not exist.
//!
//! The company as a holder ([`UnitHolder::Company`]) and the hand-over out of its stake
//! ([`IssuanceSource::Company`]) are **retired**: the variants stay so the rows and
//! events that hold them keep reading, but no new issuance may name them.
//!
//! The mirror of a mint is a **retirement** ([`UnitIssuance::retire`]) — units burnt out
//! of a holder's account with no cash leg, the way they were minted with none. The relay
//! posts `Dr SharesOutstanding / Cr <holder shares>`, so supply shrinks by exactly what
//! the holder gave up and the invariant holds as before. `units` is the magnitude on
//! every row; the source says which way the supply moved, so the console reads one
//! history — mint, hand-over (historical), retire — and the positive-digits column CHECK
//! never has to learn a sign.
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
/// An allocation is a holder in its own right rather than a user with a well-known id:
/// it has no `users` row, no cost-basis projection and no P&L to report, and giving it
/// a synthetic user would drag every investor-facing read (positions, fees, the
/// activity feed) into special-casing one UUID. Only a reserved allocation may hold —
/// see the module header and the gate in the aggregate's constructors.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum UnitHolder {
	/// An investor: units land in their `UserShares` and their `fund_positions`
	/// projection gains the issuance's cost basis.
	User(UserId),
	/// The company's own stake: units land in `CompanyShares`. No projection.
	///
	/// Retired: no new issuance may name it (the constructors refuse); the variant stays
	/// so historical rows and outbox/event-log payloads keep reading.
	#[deprecated(note = "retired: replay-only, removed after the ownership contract migration (#245)")]
	Company,
	/// A reserved allocation ([`ServiceId::is_reserved`]) holding units of a product —
	/// `fee` holding a product's fee class. Units land in the product's `FeeShares`
	/// account; the allocation's own holders own them through their units of it.
	Allocation(ServiceId),
}

impl UnitHolder {
	/// The discriminator stored in the `holder_kind` column. Keep byte-identical with
	/// `evbanking_contracts::allocation::holder` (`unit_holder_strings_are_canonical`
	/// guards this side).
	// The retired variant still has to be spelled: this is the persistence vocabulary,
	// and a stored row is read back through it.
	#[allow(deprecated)]
	pub fn kind_str(&self) -> &'static str {
		match self {
			Self::User(_) => "user",
			Self::Company => "company",
			Self::Allocation(_) => "allocation",
		}
	}

	/// The identity stored in the `holder_id` column (`None` unless a user).
	pub fn user_id(&self) -> Option<UserId> {
		match self {
			Self::User(id) => Some(*id),
			_ => None,
		}
	}

	/// The identity stored in the `holder_service` column (`None` unless an allocation).
	pub fn service_id(&self) -> Option<&ServiceId> {
		match self {
			Self::Allocation(service) => Some(service),
			_ => None,
		}
	}

	/// Reconstruct from the `(kind, holder_id, holder_service)` column triple
	/// (persistence adapter). The retired `company` kind still reads: the rows exist.
	#[allow(deprecated)]
	pub fn from_parts(kind: &str, user: Option<UserId>, service: Option<ServiceId>) -> Result<Self, DomainError> {
		match (kind, user, service) {
			("user", Some(user), None) => Ok(Self::User(user)),
			("company", None, None) => Ok(Self::Company),
			("allocation", None, Some(service)) => Ok(Self::Allocation(service)),
			_ => Err(DomainError::Validation(format!("invalid unit holder: {kind}"))),
		}
	}

	/// The holder graph, as one gate: a user may hold anything; an allocation may hold
	/// only when it is reserved and the product is not (reserved → product, one hop, so
	/// no allocation ever holds itself, another product, or another reserved one — every
	/// unit reaches a person); and the retired company holder is refused outright. The
	/// holder must also have a physical account to land on ([`Self::shares_key`]), which
	/// in phase 1 admits `fee` and refuses `fund`.
	///
	/// The aggregate's constructors run it; a use case runs it first of all, before any
	/// read, so a refused holder is refused as such and not as "holds nothing" or "no
	/// such allocation".
	#[allow(deprecated)]
	pub fn ensure_may_hold(&self, service: &ServiceId) -> Result<(), DomainError> {
		match self {
			Self::User(_) => Ok(()),
			Self::Company => Err(DomainError::Validation(
				"the company is no longer a unit holder: issue to a person or to the fee allocation".into(),
			)),
			Self::Allocation(allocation) => {
				if !allocation.is_reserved() {
					return Err(DomainError::Validation(format!("'{allocation}' is not a reserved allocation and cannot hold units")));
				}
				if service.is_reserved() {
					return Err(DomainError::Validation(format!("'{service}' is a reserved allocation and can only be held by people")));
				}
				self.shares_key(service).map(drop)
			}
		}
	}

	/// The inverse of [`Self::shares_key`], widened to the book's escrow: which product
	/// and which holder a Share-ledger account's units belong to. `BookShares` is the
	/// user's — units resting in a sell are still theirs and still count in the supply
	/// invariant — and the retired company account still answers, because a scan of the
	/// map must be able to attribute its balance until the data migration moves it.
	/// `None` for `SharesOutstanding` (the supply belongs to nobody) and every cash
	/// account.
	// The retired company account is still a row in the map: a scan reads it back as itself.
	#[allow(deprecated)]
	pub fn of_holding(key: &LedgerAccountKey) -> Option<(ServiceId, Self)> {
		match key {
			LedgerAccountKey::UserShares(service, user) | LedgerAccountKey::BookShares(service, user) => Some((service.clone(), Self::User(*user))),
			LedgerAccountKey::FeeShares(service) => Some((service.clone(), Self::Allocation(ServiceId::fee()))),
			LedgerAccountKey::CompanyShares(service) => Some((service.clone(), Self::Company)),
			LedgerAccountKey::SharesOutstanding(_)
			| LedgerAccountKey::Fund
			| LedgerAccountKey::CryptoWallet(_)
			| LedgerAccountKey::UserClaim(_)
			| LedgerAccountKey::ServiceClaim(_)
			| LedgerAccountKey::FeeRevenue
			| LedgerAccountKey::WithdrawalClearing
			| LedgerAccountKey::BankCustody
			| LedgerAccountKey::BookCash(_) => None,
		}
	}

	/// The debit-normal Share-ledger account the issued units are minted into — or
	/// retired out of.
	///
	/// A `Result`, not a total map: the only allocation with a physical holding account
	/// in a product is `fee` (`FeeShares`, the product's fee class). Any other allocation
	/// holder has no account to land on, and a stored row or payload naming one is
	/// corrupt — it cannot pass the constructors' gate — so the leg is refused rather
	/// than aliased onto someone else's shares.
	#[allow(deprecated)]
	pub fn shares_key(&self, service: &ServiceId) -> Result<LedgerAccountKey, DomainError> {
		match self {
			Self::User(user) => Ok(LedgerAccountKey::UserShares(service.clone(), *user)),
			Self::Company => Ok(LedgerAccountKey::CompanyShares(service.clone())),
			Self::Allocation(holder) if *holder == ServiceId::fee() => Ok(LedgerAccountKey::FeeShares(service.clone())),
			Self::Allocation(holder) => Err(DomainError::Validation(format!("the '{holder}' allocation has no unit account in '{service}'"))),
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

/// Where an issuance's units come from — or go. `Mint` grows supply; `Company` (retired)
/// moved units the company already held and left supply alone; `Retire` shrinks supply
/// by burning a holder's units. The ledger leg differs, the record does not, and `units`
/// is always the magnitude.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IssuanceSource {
	/// Minted in kind: `Dr <holder shares> / Cr SharesOutstanding`.
	#[default]
	Mint,
	/// Out of the company's stake: `Dr UserShares / Cr CompanyShares`. The holder is
	/// always a user.
	///
	/// Retired: there is no constructor for it any more; the variant stays so the rows
	/// and outbox/event-log payloads written with it keep reading and replaying.
	#[deprecated(note = "retired: replay-only, removed after the ownership contract migration (#245)")]
	Company,
	/// Burnt with no cash leg: `Dr SharesOutstanding / Cr <holder shares>`. The mirror
	/// of `Mint` — supply shrinks by `units`.
	Retire,
}

impl IssuanceSource {
	/// The stored/wire discriminant. Keep byte-identical with
	/// `evbanking_contracts::allocation::issuance_source`
	/// (`issuance_source_strings_are_canonical` guards this side).
	// The retired variant is still the persistence vocabulary of the rows that hold it.
	#[allow(deprecated)]
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Mint => "mint",
			Self::Company => "company",
			Self::Retire => "retire",
		}
	}

	/// Parse the stored/wire form. An unrecognized value is an error rather than a
	/// silent default, so a corrupt row never reads as a mint when it moved the
	/// company's units.
	#[allow(deprecated)]
	pub fn parse(raw: &str) -> Result<Self, DomainError> {
		match raw {
			"mint" => Ok(Self::Mint),
			"company" => Ok(Self::Company),
			"retire" => Ok(Self::Retire),
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

/// The issuance aggregate — one in-kind mint or one retirement (and, historically, one
/// hand-over out of the company's stake). Construct via [`UnitIssuance::issue`] /
/// [`UnitIssuance::retire`] (both raise [`IssuanceEvent::Issued`]) or
/// [`UnitIssuance::rehydrate`] (load from the store, no events).
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
	/// may be anything, zero included — a stake in an asset the holder already owned
	/// cost them no cash. Rejects zero `units` and a holder the graph does not admit
	/// (see the module header). Raises `Issued`.
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

	/// Burn `units` of `service` out of `holder`'s account at `nav`, with no cash leg —
	/// the reverse of [`Self::issue`]. `cost_basis` follows the same rule as a mint's
	/// (`None` is `units × nav`): it is the book value the operator is writing off, an
	/// accounting note on the row, not cash that moves. Whether the holder actually has
	/// `units` free is a ledger fact the use case reads first (Read-First) and
	/// TigerBeetle's non-negative flag on the holder's account backstops; the aggregate
	/// cannot know it. Rejects zero `units`. Raises `Issued` with `source = Retire`.
	pub fn retire(
		id: UnitIssuanceId,
		service: ServiceId,
		holder: UnitHolder,
		units: Shares,
		nav: Nav,
		cost_basis: Option<Usdt>,
		idempotency_key: IdempotencyKey,
	) -> Result<Self, DomainError> {
		Self::record(id, service, holder, IssuanceSource::Retire, units, nav, cost_basis, idempotency_key)
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
		holder.ensure_may_hold(&service)?;
		let cost_basis = match cost_basis {
			Some(basis) => basis,
			None => nav.value(units)?,
		};
		let mut issuance = Self {
			id,
			service: service.clone(),
			holder: holder.clone(),
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
	pub fn matches_request(&self, holder: &UnitHolder, source: IssuanceSource, units: Shares) -> bool {
		self.holder == *holder && self.source == source && self.units == units
	}

	pub fn id(&self) -> UnitIssuanceId {
		self.id
	}

	pub fn service(&self) -> &ServiceId {
		&self.service
	}

	pub fn holder(&self) -> &UnitHolder {
		&self.holder
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
	/// Units handed to — or, for `Retire`, taken from — a holder with no cash leg
	/// (relay, by `source`: `Mint` → `Dr <holder shares> / Cr SharesOutstanding`,
	/// `Company` (historical) → `Dr UserShares / Cr CompanyShares`, `Retire` → `Dr
	/// SharesOutstanding / Cr <holder shares>`, for `units`; then, for a user holder,
	/// `fund_positions` gains the basis and units, or for a retire sheds them pro rata).
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

	fn fee() -> UnitHolder {
		UnitHolder::Allocation(ServiceId::fee())
	}

	fn key() -> IdempotencyKey {
		IdempotencyKey::parse("issue-1").unwrap()
	}

	fn issue(holder: UnitHolder, units: &str) -> Result<UnitIssuance, DomainError> {
		UnitIssuance::issue(UnitIssuanceId::new(), svc(), holder, Shares::parse_decimal(units).unwrap(), Nav::SEED, None, key())
	}

	fn issue_into(service: ServiceId, holder: UnitHolder) -> Result<UnitIssuance, DomainError> {
		UnitIssuance::issue(UnitIssuanceId::new(), service, holder, Shares::parse_decimal("10").unwrap(), Nav::SEED, None, key())
	}

	fn retire(holder: UnitHolder, units: &str, nav: &str) -> Result<UnitIssuance, DomainError> {
		UnitIssuance::retire(
			UnitIssuanceId::new(),
			svc(),
			holder,
			Shares::parse_decimal(units).unwrap(),
			Nav::parse_decimal(nav).unwrap(),
			None,
			key(),
		)
	}

	#[test]
	#[allow(deprecated)]
	fn unit_holder_strings_are_canonical() {
		// Wire contract: these must match `evbanking_contracts::allocation::holder`.
		let user = UserId::new();
		assert_eq!(UnitHolder::User(user).kind_str(), "user");
		assert_eq!(UnitHolder::Company.kind_str(), "company");
		assert_eq!(fee().kind_str(), "allocation");
		for holder in [UnitHolder::User(user), UnitHolder::Company, fee()] {
			assert_eq!(UnitHolder::from_parts(holder.kind_str(), holder.user_id(), holder.service_id().cloned()).unwrap(), holder);
		}
		// A user without an id, a company with one, an allocation without a service, a
		// user with a service, and an unknown kind are all corrupt rows.
		assert!(UnitHolder::from_parts("user", None, None).is_err());
		assert!(UnitHolder::from_parts("company", Some(user), None).is_err());
		assert!(UnitHolder::from_parts("allocation", None, None).is_err());
		assert!(UnitHolder::from_parts("user", Some(user), Some(ServiceId::fee())).is_err());
		assert!(UnitHolder::from_parts("fund", None, None).is_err());
	}

	#[test]
	#[allow(deprecated)]
	fn a_holding_account_reads_back_as_its_product_and_holder() {
		let user = UserId::new();
		// The inverse of `shares_key` for every holder that has one...
		for holder in [UnitHolder::User(user), fee(), UnitHolder::Company] {
			let key = holder.shares_key(&svc()).unwrap();
			assert_eq!(UnitHolder::of_holding(&key), Some((svc(), holder.clone())), "{key:?}");
		}
		// ...and the book's escrow is the user's, not the book's.
		assert_eq!(UnitHolder::of_holding(&LedgerAccountKey::BookShares(svc(), user)), Some((svc(), UnitHolder::User(user))));
		// Supply and cash have no holder.
		assert_eq!(UnitHolder::of_holding(&LedgerAccountKey::SharesOutstanding(svc())), None);
		assert_eq!(UnitHolder::of_holding(&LedgerAccountKey::ServiceClaim(svc())), None);
		assert_eq!(UnitHolder::of_holding(&LedgerAccountKey::UserClaim(user)), None);
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
	#[allow(deprecated)]
	fn issuance_source_strings_are_canonical() {
		// Wire contract: these must match `evbanking_contracts::allocation::issuance_source`.
		// The retired `company` source still round-trips — the rows that hold it exist.
		assert_eq!(IssuanceSource::Mint.as_str(), "mint");
		assert_eq!(IssuanceSource::Company.as_str(), "company");
		assert_eq!(IssuanceSource::Retire.as_str(), "retire");
		for source in [IssuanceSource::Mint, IssuanceSource::Company, IssuanceSource::Retire] {
			assert_eq!(IssuanceSource::parse(source.as_str()).unwrap(), source);
			assert_eq!(serde_json::to_string(&source).unwrap(), format!("\"{}\"", source.as_str()));
		}
		assert!(IssuanceSource::parse("transfer").is_err());
		assert!(IssuanceSource::parse("burn").is_err());
	}

	#[test]
	#[allow(deprecated)]
	fn each_holder_mints_into_its_own_share_account() {
		let user = UserId::new();
		assert_eq!(UnitHolder::User(user).shares_key(&svc()).unwrap(), LedgerAccountKey::UserShares(svc(), user));
		// The fee allocation's holding in a product IS the product's fee class.
		assert_eq!(fee().shares_key(&svc()).unwrap(), LedgerAccountKey::FeeShares(svc()));
		// The retired company holder still resolves: its account is what the data
		// migration debits.
		assert_eq!(UnitHolder::Company.shares_key(&svc()).unwrap(), LedgerAccountKey::CompanyShares(svc()));
		// No other allocation has an account in a product — a payload naming one is
		// refused, never aliased onto someone's shares.
		assert!(UnitHolder::Allocation(ServiceId::fund()).shares_key(&svc()).is_err());
		assert!(UnitHolder::Allocation(ServiceId::parse("other").unwrap()).shares_key(&svc()).is_err());
	}

	#[test]
	fn the_holder_graph_is_reserved_to_product_only() {
		// reserved → product: admitted.
		assert!(issue(fee(), "10").is_ok());
		// product → product, product → reserved: a product is never a holder.
		let product = UnitHolder::Allocation(ServiceId::parse("other").unwrap());
		let err = issue(product.clone(), "10").unwrap_err();
		assert!(matches!(err, DomainError::Validation(ref m) if m.contains("not a reserved allocation")), "{err:?}");
		assert!(issue_into(ServiceId::fee(), product).is_err());
		// reserved → reserved (fee holding fund, fund holding fee, fee holding fee): the
		// graph has no cycles and no second hop — a reserved allocation is held by people.
		for (holder, service) in [(ServiceId::fee(), ServiceId::fund()), (ServiceId::fund(), ServiceId::fee()), (ServiceId::fee(), ServiceId::fee())] {
			let err = issue_into(service, UnitHolder::Allocation(holder)).unwrap_err();
			assert!(matches!(err, DomainError::Validation(ref m) if m.contains("held by people")), "{err:?}");
		}
		// A person may hold a reserved allocation — that is the whole point.
		assert!(issue_into(ServiceId::fund(), UnitHolder::User(UserId::new())).is_ok());
		// Phase 1: the fund allocation holds no product units (no account to land on).
		let err = issue(UnitHolder::Allocation(ServiceId::fund()), "10").unwrap_err();
		assert!(matches!(err, DomainError::Validation(ref m) if m.contains("no unit account")), "{err:?}");
		// The gate stands on the way out too.
		assert!(retire(UnitHolder::Allocation(ServiceId::fund()), "1", "1").is_err());
	}

	#[test]
	#[allow(deprecated)]
	fn a_company_holder_issuance_is_refused() {
		// The company is retired as a holder: the row still reads, but nothing new names it.
		for err in [issue(UnitHolder::Company, "10").unwrap_err(), retire(UnitHolder::Company, "10", "1").unwrap_err()] {
			assert!(matches!(err, DomainError::Validation(ref m) if m.contains("no longer a unit holder")), "{err:?}");
		}
		// The stored record of a company holder still rehydrates: the rows exist.
		let stored = UnitIssuance::rehydrate(UnitIssuanceSnapshot {
			id: UnitIssuanceId::new(),
			service: svc(),
			holder: UnitHolder::Company,
			source: IssuanceSource::Company,
			units: Shares::parse_decimal("13000").unwrap(),
			nav: Nav::SEED,
			cost_basis: Usdt::ZERO,
			idempotency_key: key(),
			state: IssuanceState::Applied,
		});
		assert_eq!(stored.holder(), &UnitHolder::Company);
		assert_eq!(stored.source(), IssuanceSource::Company);
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
			fee(),
			Shares::parse_decimal("800").unwrap(),
			Nav::parse_decimal("1.25").unwrap(),
			None,
			key(),
		)
		.unwrap();
		assert_eq!(defaulted.cost_basis(), Usdt::parse_decimal("1000").unwrap());
		// An explicit basis is taken as given — zero included, for an asset that cost no cash.
		let explicit = UnitIssuance::issue(UnitIssuanceId::new(), svc(), fee(), Shares::parse_decimal("800").unwrap(), Nav::SEED, Some(Usdt::ZERO), key()).unwrap();
		assert_eq!(explicit.cost_basis(), Usdt::ZERO);
	}

	#[test]
	fn issue_starts_queued_and_emits_issued() {
		let user = UserId::new();
		let mut issuance = issue(UnitHolder::User(user), "200").unwrap();
		assert_eq!(issuance.state(), IssuanceState::Queued);
		let events = issuance.drain_events();
		assert_eq!(events.len(), 1);
		assert!(matches!(&events[0], IssuanceEvent::Issued { holder: UnitHolder::User(u), .. } if *u == user));
		assert!(issuance.drain_events().is_empty());
	}

	#[test]
	fn zero_units_are_rejected() {
		assert!(issue(fee(), "0").is_err());
		assert!(retire(fee(), "0", "1").is_err());
	}

	#[test]
	fn a_retirement_is_the_mirror_of_a_mint_with_the_units_as_a_magnitude() {
		let user = UserId::new();
		for holder in [UnitHolder::User(user), fee()] {
			let mut retired = retire(holder.clone(), "500", "1.25").unwrap();
			assert_eq!(retired.holder(), &holder, "either holder can be retired from");
			assert_eq!(retired.source(), IssuanceSource::Retire);
			// Positive on the row; the source carries the direction.
			assert_eq!(retired.units(), Shares::parse_decimal("500").unwrap());
			assert_eq!(
				retired.cost_basis(),
				Usdt::parse_decimal("625").unwrap(),
				"the written-off basis defaults to units × NAV like a mint"
			);
			assert_eq!(retired.state(), IssuanceState::Queued);
			let events = retired.drain_events();
			assert!(matches!(
				events.as_slice(),
				[IssuanceEvent::Issued {
					source: IssuanceSource::Retire,
					holder: h,
					..
				}] if *h == holder
			));
		}
		// A retirement under a mint's key is a different request, not a retry — one
		// grows supply, the other shrinks it.
		let units = Shares::parse_decimal("500").unwrap();
		let mint = issue(fee(), "500").unwrap();
		assert!(!mint.matches_request(&fee(), IssuanceSource::Retire, units));
	}

	#[test]
	#[allow(deprecated)]
	fn a_retry_matches_on_holder_source_and_units_only() {
		let user = UserId::new();
		let units = Shares::parse_decimal("200").unwrap();
		let issuance = issue(UnitHolder::User(user), "200").unwrap();
		assert!(issuance.matches_request(&UnitHolder::User(user), IssuanceSource::Mint, units));
		assert!(!issuance.matches_request(&fee(), IssuanceSource::Mint, units));
		assert!(!issuance.matches_request(&UnitHolder::User(user), IssuanceSource::Mint, Shares::parse_decimal("201").unwrap()));
		// The same key naming a mint and then a (historical) hand-over of the company's
		// stake is a different request, not a retry: one grows supply, the other does not.
		assert!(!issuance.matches_request(&UnitHolder::User(user), IssuanceSource::Company, units));
	}

	#[test]
	#[allow(deprecated)]
	fn an_event_written_before_the_source_existed_reads_as_a_mint() {
		// The permanent event log holds `Issued` payloads with no `source`; every one of
		// them was a mint, and a stored fact must stay readable as the vocabulary grows —
		// including the retired company holder those early mints went to.
		let json = format!(
			r#"{{"type":"issued","issuance_id":"{}","service":"service_arb","holder":{{"kind":"company"}},"units":"10","nav":"1","cost_basis":"10"}}"#,
			UnitIssuanceId::new()
		);
		let IssuanceEvent::Issued { source, holder, .. } = serde_json::from_str(&json).unwrap();
		assert_eq!(source, IssuanceSource::Mint);
		assert_eq!(holder, UnitHolder::Company);
	}

	#[test]
	fn event_round_trips_through_json_for_both_holders() {
		for holder in [UnitHolder::User(UserId::new()), fee()] {
			let mut issuance = issue(holder.clone(), "10").unwrap();
			let event = issuance.drain_events().pop().unwrap();
			let json = serde_json::to_string(&event).unwrap();
			let back: IssuanceEvent = serde_json::from_str(&json).unwrap();
			let IssuanceEvent::Issued { holder: back_holder, .. } = back;
			assert_eq!(back_holder, holder);
		}
		// The allocation holder is self-describing, carrying its slug.
		assert_eq!(serde_json::to_string(&fee()).unwrap(), r#"{"kind":"allocation","id":"fee"}"#);
	}
}
