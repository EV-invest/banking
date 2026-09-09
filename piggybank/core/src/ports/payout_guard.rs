//! The payout policy gate — the global read-only kill-switch and the cross-plane
//! freeze on a withdrawal's owner, read fresh at **dispatch** time.
//!
//! Neither flag is a domain aggregate: `read_only` is a single-row config toggle and
//! `frozen`/`disabled` are bridge-mirrored columns the [`User`](domain::users::User)
//! aggregate deliberately doesn't model (see [`super::UserRepository`]'s
//! `IssuanceTarget` doc). This port exists so [`application::withdrawals::dispatch_withdrawal`](crate::application::withdrawals::dispatch_withdrawal)
//! can enforce the SAME two gates [`services::support::unfrozen_caller`](crate::services::support)
//! enforces at the sync RPC boundary, without pulling Postgres into the application
//! layer.

use async_trait::async_trait;
use domain::{error::DomainError, users::UserId};

#[async_trait]
pub trait PayoutGuard: Send + Sync {
	/// The global read-only kill-switch — every outflow, sync or async, honors it.
	async fn is_read_only(&self) -> Result<bool, DomainError>;

	/// Whether `user`'s banking row is blocked from moving money: a concierge
	/// SUSPENDED (mirrored `frozen`) OR a banking-side disable (`status =
	/// 'disabled'`) — the same fold `services::support::unfrozen_caller` reads.
	async fn is_frozen(&self, user: UserId) -> Result<bool, DomainError>;
}
