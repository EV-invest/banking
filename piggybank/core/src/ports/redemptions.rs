//! Persistence + read port for the [`Redemption`] aggregate.
//!
//! Mirrors [`WithdrawalRepository`](super::WithdrawalRepository): each command is
//! internally atomic and row-locked — load `FOR UPDATE`, apply the aggregate command
//! inside the lock, then persist the transition with the drained events. The relay
//! reserves the pending burn, then (at settle) posts the cash payout and the burn
//! (Write-Last). `settle` also reduces the user's `fund_positions` cost basis
//! proportionally (average cost) for P&L — dividing by the position's own tracked units
//! (decremented under the same lock), so concurrent settles compound deterministically
//! instead of each dividing by the relay-lagging pre-burn TigerBeetle balance.

use async_trait::async_trait;
use domain::{
	architecture::{Reader, Repository},
	balance::ServiceId,
	error::DomainError,
	money::{Nav, Shares},
	redemptions::{Redemption, RedemptionId},
	users::UserId,
};

#[async_trait]
pub trait RedemptionRepository: Repository<Aggregate = Redemption> + Reader<Aggregate = Redemption> {
	/// Persist a brand-new redemption (`Queued`) + its `Requested` event — the relay
	/// reserves a pending burn of the units. Takes a `FOR UPDATE` lock on the user's
	/// `fund_positions` row to serialize concurrent requests at the Postgres layer
	/// (TigerBeetle's non-negative flag is the actual over-redeem backstop).
	async fn open(&self, redemption: &mut Redemption) -> Result<(), DomainError>;

	/// Settle a queued redemption at the settle-time `nav`: apply [`Redemption::settle`]
	/// under the row lock, persist + drain the `Settled` event (the relay pays the cash
	/// and posts the burn), and reduce the position's cost basis proportionally — dividing
	/// by the position's own projection-tracked units (locked + decremented in the same tx,
	/// so concurrent settles compound). Idempotent; returns the aggregate.
	async fn settle(&self, id: RedemptionId, nav: Nav) -> Result<Redemption, DomainError>;

	/// Fail a queued redemption (operator): apply [`Redemption::fail`] under the lock,
	/// persist + drain the `Failed` event (the relay voids the burn, returning the units).
	async fn fail(&self, id: RedemptionId) -> Result<Redemption, DomainError>;

	/// Cancel a queued redemption (user): apply [`Redemption::cancel`] under the lock,
	/// persist + drain the `Cancelled` event (the relay voids the burn). Idempotent.
	async fn cancel(&self, id: RedemptionId) -> Result<Redemption, DomainError>;

	/// Load a redemption by id (no lock; for queries).
	async fn find_by_id(&self, id: RedemptionId) -> Result<Option<Redemption>, DomainError>;

	/// A user's redemptions (projection), newest first.
	async fn list_by_user(&self, user: UserId) -> Result<Vec<Redemption>, DomainError>;

	/// The cross-user queue of redemptions awaiting settle (state `queued`), oldest
	/// first — the operator "clear the queue" read. Joins the mirrored identity email.
	async fn list_queued(&self) -> Result<Vec<QueuedRedemption>, DomainError>;
}
/// A queued redemption for the operator "clear the queue" surface — a lightweight read
/// projection that also carries the mirrored identity `email` and DB-managed
/// `created_at`, neither of which the [`Redemption`] aggregate models.
pub struct QueuedRedemption {
	pub id: RedemptionId,
	pub user_id: UserId,
	pub email: String,
	pub service: ServiceId,
	pub units: Shares,
	/// Unix seconds the redemption was requested (for the "age" column).
	pub created_at: i64,
}
