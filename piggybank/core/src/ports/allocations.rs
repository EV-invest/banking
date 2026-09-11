//! Persistence + read port for the [`Allocation`] aggregate — the registry of
//! investable products.
//!
//! Mirrors [`RedemptionRepository`](super::RedemptionRepository): each command is
//! internally atomic and row-locked — load `FOR UPDATE`, apply the aggregate command
//! inside the lock, persist the transition with the drained events. Unlike the money
//! aggregates the drained events go to `event_log` ONLY (`relay = false`): an
//! allocation moves no value, so the relay has nothing to post.
//!
//! [`find`] and [`find_for`] are on the hot path — every subscribe and redeem resolves
//! its service through one of them before touching money — so they take no lock and
//! stay a single indexed read on the natural key. `find_for` additionally joins the
//! caller's grant, because the subscribe gate needs the level the caller *effectively*
//! holds and answering that in two round trips would be a second trip for one question.
//!
//! Per-investor grants live in their own table beside the aggregate rather than inside
//! it: they are keyed by user, so loading every grant onto every `find` would be waste,
//! and the aggregate needs them only to raise the audit facts a grant or revoke leaves
//! behind. The effective level is therefore **computed, never stored** — by
//! [`AllocationAccess::effective`] over the row's default and the caller's grant.
//!
//! [`find`]: AllocationRegistry::find
//! [`find_for`]: AllocationRegistry::find_for

use async_trait::async_trait;
use domain::{
	allocations::{Allocation, AllocationAccess, AllocationIcon},
	architecture::{Reader, Repository},
	balance::ServiceId,
	error::DomainError,
	money::Shares,
	users::UserId,
};

#[async_trait]
pub trait AllocationRegistry: Repository<Aggregate = Allocation> + Reader<Aggregate = Allocation> {
	/// Persist a brand-new allocation (`draft`, at the default access level) + its
	/// `Registered` event. The unique `service` key makes a double registration a
	/// [`DomainError::Conflict`] rather than a silent overwrite — re-registering a live
	/// product must never reset it.
	async fn register(&self, allocation: &mut Allocation) -> Result<(), DomainError>;

	/// Replace the presentation fields — title, summary and icon — under the row lock:
	/// load `FOR UPDATE`, apply [`Allocation::update_details`], persist + drain.
	/// `NotFound` if unregistered.
	///
	/// `icon` is `None` when the caller said nothing about the icon, and the stored pick
	/// survives the edit — see [`Allocation::update_details`] for why the field alone
	/// carries that distinction.
	async fn update_details(&self, service: &ServiceId, title: &str, summary: &str, icon: Option<AllocationIcon>) -> Result<Allocation, DomainError>;

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

	/// Set the default access level under the row lock, applying
	/// [`Allocation::set_access`] (idempotent). Grants are untouched. `NotFound` if
	/// unregistered.
	async fn set_access(&self, service: &ServiceId, access: AllocationAccess) -> Result<Allocation, DomainError>;

	/// Raise `user` to `level` on `service`, recording `granted_by`. A repeat grant for
	/// the same user overwrites the level; one that changes nothing raises no event.
	/// Taken under the allocation's row lock so the audit fact and the row agree.
	/// `NotFound` if unregistered; `Validation` for a level a grant may not carry.
	async fn grant_access(&self, service: &ServiceId, user: UserId, level: AllocationAccess, granted_by: UserId) -> Result<AllocationAccessGrant, DomainError>;

	/// Take `user`'s grant on `service` back, recording `revoked_by`. Idempotent: a
	/// grant that does not stand is not an error and raises nothing. `NotFound` if the
	/// allocation itself is unregistered.
	async fn revoke_access(&self, service: &ServiceId, user: UserId, revoked_by: UserId) -> Result<(), DomainError>;

	/// Every standing grant on `service`, ordered by user. `NotFound` if unregistered
	/// — an empty list means "no grants", never "no such product".
	async fn list_grants(&self, service: &ServiceId) -> Result<Vec<AllocationAccessGrant>, DomainError>;

	/// Resolve one allocation by its natural key, caller-agnostic. `None` means the
	/// service was never registered — which is exactly what the redeem gate turns into
	/// a refusal. For anything a caller's access decides, use [`Self::find_for`].
	async fn find(&self, service: &ServiceId) -> Result<Option<Allocation>, DomainError>;

	/// Resolve one allocation as `caller` sees it: the aggregate plus the level they
	/// effectively hold. Unfiltered — a `hidden` result is returned, and it is the
	/// application layer's decision what a caller without `AllocationManage` is told.
	async fn find_for(&self, service: &ServiceId, caller: UserId) -> Result<Option<AllocationRecord>, DomainError>;

	/// The catalog as `caller` sees it, ordered by `service`. The default is the
	/// investor-facing set: `open` products whose effective level for the caller is
	/// at least `view`. `include_unlisted` lifts both filters — every row in every
	/// state at every level, each still carrying the caller's honest effective level.
	async fn list_for(&self, caller: UserId, include_unlisted: bool) -> Result<Vec<AllocationRecord>, DomainError>;
}

/// A catalog row as one caller sees it — the aggregate, the level that caller
/// effectively holds on it, and the DB-stamped timestamps the domain deliberately does
/// not model (it stays clock-free).
#[derive(Debug)]
pub struct AllocationRecord {
	pub allocation: Allocation,
	/// [`AllocationAccess::effective`] over the product's default and the caller's
	/// grant. Honest for every caller, an `AllocationManage` holder included: their
	/// permission reads the product, it does not invest in it.
	pub caller_access: AllocationAccess,
	/// Unix seconds the allocation was registered.
	pub created_at: i64,
	/// Unix seconds of the last details/state/access change.
	pub updated_at: i64,
}

/// One investor raised above a product's default level.
#[derive(Debug)]
pub struct AllocationAccessGrant {
	pub service: ServiceId,
	pub user_id: UserId,
	pub level: AllocationAccess,
	pub granted_by: UserId,
	/// Unix seconds the grant was (last) written.
	pub granted_at: i64,
}
