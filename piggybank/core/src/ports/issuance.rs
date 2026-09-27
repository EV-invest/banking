//! Persistence + read port for the [`UnitIssuance`] aggregate — an operator's in-kind
//! mint.
//!
//! An issuance is an immutable record with one relay-driven transition (`queued` →
//! `applied`), so the write side is a single `issue`: insert the row and drain its
//! `Issued` event to the outbox in one transaction. The relay then posts the mint
//! (Write-Last) and stamps the row `applied` — together with the `fund_positions`
//! cost-basis projection when the holder is a user — after the leg lands, the same
//! discipline as a subscription's projection, so a parked mint never leaves a
//! phantom basis or a row claiming units that were never minted.
//!
//! The unique `(service, idempotency_key)` is the retry contract, enforced where a
//! race can only be settled: two concurrent sends of one request both insert, one hits
//! the constraint, and the adapter hands it the row the other one won with.
//!
//! **History the domain no longer names.** Two rows in production predate #245: the
//! 13 000 `service_arb` units minted to the company stake (`holder_kind = 'company'`) and
//! their hand-over to a person (`source = 'company'`). The domain has no company holder
//! and no hand-over source any more, so a lookup that lands on one of them answers
//! [`StoredIssuance::RetiredCompany`] — a read-only record of what happened — rather than
//! failing or inventing a live [`UnitIssuance`] for it. Nothing writes that shape: the
//! `unit_issuances` CHECKs refuse it since migration 0047 (`NOT VALID`, so the history
//! stays), and there is no constructor for it here.

use async_trait::async_trait;
use domain::{
	architecture::Repository,
	balance::ServiceId,
	error::DomainError,
	issuance::{IdempotencyKey, UnitIssuance, UnitIssuanceId},
	money::Shares,
	users::UserId,
};

#[async_trait]
pub trait UnitIssuanceRepository: Repository<Aggregate = UnitIssuance> {
	/// Persist a brand-new issuance and drain its `Issued` event — atomically — unless
	/// a row already stands for its `(service, idempotency_key)`, in which case nothing
	/// is written and that row is returned. The aggregate is drained only on a genuine
	/// insert, so a lost race never leaves an event with no row behind it. A key already
	/// taken by a [`StoredIssuance::RetiredCompany`] row is a [`DomainError::Conflict`]:
	/// that row is not a request this call could be a retry of.
	async fn issue(&self, issuance: &mut UnitIssuance) -> Result<IssueOutcome, DomainError>;

	/// The issuance recorded under `key` for `service`, if any — the idempotency read
	/// the use case runs before minting a new id.
	async fn find_by_key(&self, service: &ServiceId, key: &IdempotencyKey) -> Result<Option<StoredIssuance>, DomainError>;

	/// One issuance by id.
	async fn find_by_id(&self, id: UnitIssuanceId) -> Result<Option<StoredIssuance>, DomainError>;

	/// Units of `mint` issuances on `service` still `queued` — what the supply will grow
	/// by once the relay posts them.
	async fn queued_mint_units(&self, service: &ServiceId) -> Result<Shares, DomainError>;
}

/// What [`UnitIssuanceRepository::issue`] did: wrote the caller's aggregate, or found
/// the earlier row its key already names.
pub enum IssueOutcome {
	/// The aggregate was inserted and its event drained; `created_at` is the DB stamp.
	Recorded(UnitIssuanceRecord),
	/// The key was already taken for this service — the standing row, untouched.
	Existing(UnitIssuanceRecord),
}

impl IssueOutcome {
	pub fn into_record(self) -> UnitIssuanceRecord {
		match self {
			Self::Recorded(record) | Self::Existing(record) => record,
		}
	}
}

/// A `unit_issuances` row as a lookup finds it: a live issuance, or one of the two
/// shapes #245 retired, kept as history (see the module header).
#[derive(Debug)]
pub enum StoredIssuance {
	Live(UnitIssuanceRecord),
	RetiredCompany(RetiredCompanyIssuance),
}

impl StoredIssuance {
	/// The live issuance, or `None` for a historical row.
	pub fn live(self) -> Option<UnitIssuanceRecord> {
		match self {
			Self::Live(record) => Some(record),
			Self::RetiredCompany(_) => None,
		}
	}
}

/// A pre-#245 issuance involving the retired company stake — read-only history. It has no
/// aggregate, emits nothing and plans no leg: its units were posted long ago (onto
/// `shares_company:<svc>`, code 63, and from there to the named person), and the person's
/// `UserShares` and `fund_positions` already carry them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetiredCompanyIssuance {
	pub id: UnitIssuanceId,
	pub service: ServiceId,
	pub movement: RetiredCompanyMovement,
	/// The magnitude, as on every issuance row.
	pub units: Shares,
	/// Unix seconds the row was recorded.
	pub created_at: i64,
	/// Unix seconds its leg posted; `None` if it never did.
	pub applied_at: Option<i64>,
}

/// Which of the two retired shapes a [`RetiredCompanyIssuance`] is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetiredCompanyMovement {
	/// `holder_kind = 'company'`, `source = 'mint'`: units minted into the company stake.
	MintedToCompany,
	/// `holder_kind = 'user'`, `source = 'company'`: units handed out of the company stake
	/// to this person (supply unchanged).
	HandedTo(UserId),
}

/// An issuance as stored — the aggregate plus the DB-stamped timestamps the domain
/// deliberately does not model.
#[derive(Debug)]
pub struct UnitIssuanceRecord {
	pub issuance: UnitIssuance,
	/// Unix seconds the issuance was recorded.
	pub created_at: i64,
	/// Unix seconds the relay posted the mint; `None` while still `queued`.
	pub applied_at: Option<i64>,
}
