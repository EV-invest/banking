//! Allocation registry use cases — the catalog of investable products.
//!
//! Pure control plane: nothing here touches TigerBeetle or the relay, so no handler
//! notifies it. The write side is Admin/Owner-gated at the service boundary
//! (`Permission::AllocationManage`); the read side is open to any authenticated user,
//! with the unlisted (draft/closed) rows behind the same permission.
//!
//! [`require_subscribable`] / [`require_redeemable`] are the reason the context exists:
//! they are the gate the fund use cases run before any money moves, turning "this slug
//! parses" into "an operator registered and opened this product".

use domain::{
	allocations::{Allocation, AllocationId},
	balance::ServiceId,
	error::DomainError,
};

use crate::ports::allocations::{AllocationRecord, AllocationRegistry};

/// Register `service` as a new investable product, in `draft`. A slug already in the
/// registry is a [`DomainError::Conflict`] — registration never silently overwrites a
/// live product's title or state.
pub async fn register(allocations: &dyn AllocationRegistry, service: ServiceId, title: &str, summary: &str) -> Result<Allocation, DomainError> {
	let mut allocation = Allocation::register(AllocationId::new(), service, title, summary)?;
	allocations.register(&mut allocation).await?;
	Ok(allocation)
}

/// Replace an allocation's presentation fields. State and identity are untouched.
pub async fn update_details(allocations: &dyn AllocationRegistry, service: &ServiceId, title: &str, summary: &str) -> Result<Allocation, DomainError> {
	allocations.update_details(service, title, summary).await
}

/// Open an allocation for subscriptions (idempotent).
pub async fn open(allocations: &dyn AllocationRegistry, service: &ServiceId) -> Result<Allocation, DomainError> {
	allocations.open(service).await
}

/// Close an allocation to new subscriptions (idempotent). Redemptions keep working —
/// see [`Allocation::ensure_redeemable`].
pub async fn close(allocations: &dyn AllocationRegistry, service: &ServiceId) -> Result<Allocation, DomainError> {
	allocations.close(service).await
}

/// One allocation by service id, in any state. `NotFound` if never registered.
pub async fn get(allocations: &dyn AllocationRegistry, service: &ServiceId) -> Result<Allocation, DomainError> {
	allocations.find(service).await?.ok_or_else(|| DomainError::NotFound {
		entity: "allocation",
		id: service.to_string(),
	})
}

/// The catalog. `include_unlisted` adds draft/closed rows (permission-gated at the
/// boundary); the default is the investor-facing open set.
pub async fn list(allocations: &dyn AllocationRegistry, include_unlisted: bool) -> Result<Vec<AllocationRecord>, DomainError> {
	allocations.list(include_unlisted).await
}

/// Resolve `service` and assert it takes new money. The single gate standing between a
/// well-formed slug and a fund existing: an unregistered service is `NotFound`, a
/// draft/closed one a validation error. Every subscribe runs this **before** reading a
/// balance or pricing a NAV, so an unregistered slug never reaches the ledger at all.
pub async fn require_subscribable(allocations: &dyn AllocationRegistry, service: &ServiceId) -> Result<(), DomainError> {
	get(allocations, service).await?.ensure_subscribable()
}

/// Resolve `service` and assert investors can still exit it. Deliberately laxer than
/// [`require_subscribable`]: a `closed` allocation passes, because refusing here would
/// lock units inside a wound-down product.
pub async fn require_redeemable(allocations: &dyn AllocationRegistry, service: &ServiceId) -> Result<(), DomainError> {
	get(allocations, service).await?.ensure_redeemable()
}
