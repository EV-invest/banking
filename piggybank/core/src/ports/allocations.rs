//! Persistence + read port for the [`Allocation`] aggregate.
//!
//! Each command method is internally atomic — it loads under a row lock
//! (`SELECT … FOR UPDATE`), applies the aggregate's command **inside the lock**, and
//! persists the single state transition together with the drained events
//! (`event_log` + `outbox`) in one transaction (the ACID point). The aggregate is
//! the single authority on a transition's validity (the revoke rule lives there);
//! callers do only the cheap boundary check first, never a second `sharers` read.

use async_trait::async_trait;
use domain::{
	allocations::{Allocation, AllocationId},
	architecture::{Reader, Repository},
	error::DomainError,
	users::UserId,
};

#[async_trait]
pub trait AllocationRepository: Repository<Aggregate = Allocation> + Reader<Aggregate = Allocation> {
	/// Persist a brand-new allocation and drain its `Opened` event to the event log
	/// + outbox, atomically.
	async fn open(&self, allocation: &mut Allocation) -> Result<(), DomainError>;

	/// Revoke a user's stake atomically: load `FOR UPDATE`, apply
	/// [`Allocation::revoke_by_user`] under the lock, persist the
	/// `active → revoked` transition (`UPDATE … WHERE state = 'active'`, asserting one
	/// row) + the event. Idempotent; returns the current aggregate. The domain's
	/// `Forbidden`/`Conflict` propagate unchanged.
	async fn revoke_user_stake(&self, id: AllocationId, user: UserId) -> Result<Allocation, DomainError>;

	/// Load an allocation by id (no lock; for queries).
	async fn find_by_id(&self, id: AllocationId) -> Result<Option<Allocation>, DomainError>;

	/// A user's allocations (projection), newest first.
	async fn list_by_user(&self, user: UserId) -> Result<Vec<Allocation>, DomainError>;
}
