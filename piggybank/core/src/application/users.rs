//! `users` use cases — the self-service profile update and the operator
//! revoke/disable commands.
//!
//! Each hands the whole mutation to a single row-locked repository command (see
//! [`UserRepository`]): the load (`FOR UPDATE`), the aggregate transition, and the
//! event drain are one atomic unit, so a stale read can never overwrite a concurrent
//! admin transition. Shared by the gRPC service and the auth task's `RevokeAll`
//! ([`super::auth_sync`]).

use domain::{
	error::DomainError,
	users::{ProfileFields, User, UserId},
};

use crate::ports::UserRepository;

/// Apply a self-service profile update. Returns the updated aggregate.
pub async fn update_profile(users: &dyn UserRepository, id: UserId, fields: ProfileFields) -> Result<User, DomainError> {
	users.update_profile(id, fields).await
}

/// Invalidate every outstanding token for `id` (bumps `token_version`). Returns the
/// updated aggregate (carrying the new version).
pub async fn revoke_tokens(users: &dyn UserRepository, id: UserId) -> Result<User, DomainError> {
	users.revoke_tokens(id).await
}

/// Disable the account (idempotent) — folds into the money-op and issuance gates.
pub async fn disable_user(users: &dyn UserRepository, id: UserId) -> Result<User, DomainError> {
	users.disable(id).await
}
