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

use async_trait::async_trait;
use domain::{
	architecture::Repository,
	balance::ServiceId,
	error::DomainError,
	issuance::{IdempotencyKey, UnitIssuance, UnitIssuanceId},
};

#[async_trait]
pub trait UnitIssuanceRepository: Repository<Aggregate = UnitIssuance> {
	/// Persist a brand-new issuance and drain its `Issued` event — atomically — unless
	/// a row already stands for its `(service, idempotency_key)`, in which case nothing
	/// is written and that row is returned. The aggregate is drained only on a genuine
	/// insert, so a lost race never leaves an event with no row behind it.
	async fn issue(&self, issuance: &mut UnitIssuance) -> Result<IssueOutcome, DomainError>;

	/// The issuance recorded under `key` for `service`, if any — the idempotency read
	/// the use case runs before minting a new id.
	async fn find_by_key(&self, service: &ServiceId, key: &IdempotencyKey) -> Result<Option<UnitIssuanceRecord>, DomainError>;

	/// One issuance by id.
	async fn find_by_id(&self, id: UnitIssuanceId) -> Result<Option<UnitIssuanceRecord>, DomainError>;
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
