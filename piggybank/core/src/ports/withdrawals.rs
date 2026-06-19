//! Persistence + read port for the [`Withdrawal`] aggregate.
//!
//! Mirrors [`AllocationRepository`](super::AllocationRepository): each command is
//! internally atomic and row-locked — load `FOR UPDATE`, apply the aggregate command
//! inside the lock (the aggregate is the single authority on a transition's
//! validity), then persist the transition together with the drained events
//! (`event_log` + `outbox`) in one transaction. The relay then reserves/settles/voids
//! the money in TigerBeetle (Write-Last).

use async_trait::async_trait;
use domain::{
	architecture::{Reader, Repository},
	error::DomainError,
	money::TxRef,
	users::UserId,
	withdrawals::{Withdrawal, WithdrawalId},
};

#[async_trait]
pub trait WithdrawalRepository: Repository<Aggregate = Withdrawal> + Reader<Aggregate = Withdrawal> {
	/// Persist a brand-new withdrawal and drain its `Requested` event — the relay
	/// reserves the net + fee legs against the user's claim — atomically.
	async fn open(&self, withdrawal: &mut Withdrawal) -> Result<(), DomainError>;

	/// Settle a pending withdrawal (it has the required confirmations): apply
	/// [`Withdrawal::settle`] under the row lock, persist + drain the `Settled` event
	/// (the relay posts the reservations). Idempotent; returns the current aggregate.
	async fn settle(&self, id: WithdrawalId, tx_ref: TxRef) -> Result<Withdrawal, DomainError>;

	/// Fail a pending withdrawal (it never reached the chain): apply
	/// [`Withdrawal::fail`] under the row lock, persist + drain the `Failed` event (the
	/// relay voids the reservations, refunding the user). Idempotent.
	async fn fail(&self, id: WithdrawalId) -> Result<Withdrawal, DomainError>;

	/// Load a withdrawal by id (no lock; for queries).
	async fn find_by_id(&self, id: WithdrawalId) -> Result<Option<Withdrawal>, DomainError>;

	/// A user's withdrawals (projection), newest first.
	async fn list_by_user(&self, user: UserId) -> Result<Vec<Withdrawal>, DomainError>;
}
