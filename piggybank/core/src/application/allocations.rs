//! Allocation use cases — open a user stake, revoke it, list a user's allocations.
//!
//! The boundary (gRPC) does the cheap "are you this user?" check; the *stateful*
//! rule (sole sharer, owner is the fund, still active) lives in the aggregate,
//! applied under the row lock inside [`AllocationRepository::revoke_user_stake`].

use domain::{
	allocations::{Allocation, AllocationId},
	balance::{LedgerAccountKey, ServiceId},
	error::DomainError,
	money::{Network, Usdt},
	users::UserId,
};
use tokio::sync::Notify;

use crate::ports::{AllocationRepository, ledger::Ledger};

/// A user directs `amount` of their network claim at `service`. Read-First confirms
/// the claim covers it (the TB `DebitsMustNotExceedCredits` flag is the backstop),
/// then records the allocation; the relay moves `USER → SERVICE`.
pub async fn allocate_user_stake(
	allocations: &dyn AllocationRepository,
	ledger: &dyn Ledger,
	relay: &Notify,
	user: UserId,
	service: ServiceId,
	network: Network,
	amount: Usdt,
) -> Result<Allocation, DomainError> {
	let balance = ledger.balance(&LedgerAccountKey::UserClaim(user, network)).await?;
	if balance.posted < amount {
		return Err(DomainError::Validation("insufficient balance to allocate".into()));
	}
	let mut allocation = Allocation::open_user_stake(AllocationId::new(), user, service, network, amount)?;
	allocations.open(&mut allocation).await?;
	relay.notify_one();
	Ok(allocation)
}

/// Revoke a user's own stake. The aggregate enforces "owner is the fund and you are
/// the sole sharer" under the lock; idempotent if already revoked.
pub async fn revoke_user_stake(allocations: &dyn AllocationRepository, relay: &Notify, id: AllocationId, user: UserId) -> Result<Allocation, DomainError> {
	let allocation = allocations.revoke_user_stake(id, user).await?;
	relay.notify_one();
	Ok(allocation)
}

/// A user's allocations (projection), newest first.
pub async fn list_user_allocations(allocations: &dyn AllocationRepository, user: UserId) -> Result<Vec<Allocation>, DomainError> {
	allocations.list_by_user(user).await
}
