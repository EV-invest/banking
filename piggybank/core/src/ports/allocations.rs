//! Persistence + read port for the [`Allocation`] aggregate — the registry of
//! investable products.
//!
//! Mirrors [`RedemptionRepository`](super::RedemptionRepository): each command is
//! internally atomic and row-locked — load `FOR UPDATE`, apply the aggregate command
//! inside the lock, persist the transition with the drained events. Unlike the money
//! aggregates the drained events go to `event_log` ONLY (`relay = false`): an
//! allocation moves no value, so the relay has nothing to post.
//!
//! [`find`] is on the hot path — every subscribe and redeem resolves its service
//! through it before touching money — so it takes no lock and stays a single indexed
//! read on the natural key.

use async_trait::async_trait;
use domain::{
	allocations::Allocation,
	architecture::{Reader, Repository},
	balance::ServiceId,
	error::DomainError,
	money::Shares,
};

#[async_trait]
pub trait AllocationRegistry: Repository<Aggregate = Allocation> + Reader<Aggregate = Allocation> {
	/// Persist a brand-new allocation (`draft`) + its `Registered` event. The unique
	/// `service` key makes a double registration a [`DomainError::Conflict`] rather
	/// than a silent overwrite — re-registering a live product must never reset it.
	async fn register(&self, allocation: &mut Allocation) -> Result<(), DomainError>;

	/// Replace the presentation fields under the row lock: load `FOR UPDATE`, apply
	/// [`Allocation::update_details`], persist + drain. `NotFound` if unregistered.
	async fn update_details(&self, service: &ServiceId, title: &str, summary: &str) -> Result<Allocation, DomainError>;

	/// Resize the authorised unit supply under the row lock, applying
	/// [`Allocation::set_unit_cap`]. Its own command rather than a field on
	/// [`Self::update_details`] because it gates money: it raises its own event, so the
	/// audit answers "who resized this product" without diffing title edits.
	/// `NotFound` if unregistered.
	async fn set_unit_cap(&self, service: &ServiceId, unit_cap: Shares) -> Result<Allocation, DomainError>;

	/// Open the allocation for subscriptions under the row lock (idempotent — an
	/// already-open one drains no event). `NotFound` if unregistered.
	async fn open(&self, service: &ServiceId) -> Result<Allocation, DomainError>;

	/// Close it to new subscriptions under the row lock (idempotent). Redemptions are
	/// unaffected by design. `NotFound` if unregistered.
	async fn close(&self, service: &ServiceId) -> Result<Allocation, DomainError>;

	/// Resolve one allocation by its natural key. `None` means the service was never
	/// registered — which is exactly what the subscribe gate turns into a refusal.
	async fn find(&self, service: &ServiceId) -> Result<Option<Allocation>, DomainError>;

	/// The catalog, ordered by `service`. `include_unlisted` adds `draft` and `closed`
	/// entries; the default is the investor-facing `open` set.
	async fn list(&self, include_unlisted: bool) -> Result<Vec<AllocationRecord>, DomainError>;
}

/// A catalog row — the aggregate plus the DB-stamped timestamps it deliberately does
/// not model (the domain stays clock-free).
pub struct AllocationRecord {
	pub allocation: Allocation,
	/// Unix seconds the allocation was registered.
	pub created_at: i64,
	/// Unix seconds of the last details/state change.
	pub updated_at: i64,
}
