//! Cross-cutting helpers shared by every context service: the auth gates
//! (`caller_id`/`require_admin`), the domain→status error mapper, id parsers, and
//! the small wire-shape utilities. Each context module ([`super::users`],
//! [`super::balance`], [`super::funds`], [`super::wallet`]) owns its own proto
//! mappers; only this genuinely shared surface lives here.
//!
//! `Result<_, Status>` is tonic's mandated handler signature; `Status` is a large
//! type we don't control, so the large-err lint does not apply in this module.
#![allow(clippy::result_large_err)]

use std::time::{SystemTime, UNIX_EPOCH};

use domain::{
	authz::{Permission, Role, grants},
	error::DomainError,
	redemptions::RedemptionId,
	users::UserId,
	withdrawals::WithdrawalId,
};
use evbanking_auth::claims_of;
use tonic::{Request, Status};
use uuid::Uuid;

use crate::AppState;

/// The authenticated caller's own user id (from the access-token `sub`).
///
/// Self-service RPCs act *as a user*, so only a `typ=access` token qualifies — a
/// `typ=service` token (an inter-service principal) is rejected here, matching the
/// authz matrix, independent of whether its `sub` happens to parse as a UUID.
pub(super) fn caller_id<T>(request: &Request<T>) -> Result<UserId, Status> {
	let claims = claims_of(request).ok_or_else(|| Status::unauthenticated("missing claims"))?;
	if !claims.is_access() {
		return Err(Status::permission_denied("access token required"));
	}
	parse_user_id(&claims.sub)
}

/// Gate a money-moving RPC on the cross-plane freeze flag: reject if the caller's banking
/// row was frozen by a concierge SUSPENDED lifecycle event (see
/// [`infrastructure::bridge`](crate::infrastructure::bridge)). Returns the caller's id on
/// success so the handler keeps its existing `let user = ...?` shape. A control-plane read
/// failure fails CLOSED (UNAVAILABLE) — a money op never proceeds when the gate can't be read.
pub(super) async fn unfrozen_caller<T>(state: &AppState, request: &Request<T>) -> Result<UserId, Status> {
	let user = caller_id(request)?;
	// Global read-only kill-switch: every user-INITIATED money mutation (withdraw /
	// subscribe / redeem) routes through here, so this is the single choke point that
	// pauses outflows. Inbound on-chain deposit crediting (the deposit watchers) is
	// deliberately NOT gated — funds already on-chain are still credited. Fails CLOSED.
	match crate::infrastructure::operations::is_read_only(&state.pool).await {
		Ok(false) => {}
		Ok(true) => return Err(Status::failed_precondition("money movements are temporarily paused (read-only mode)")),
		Err(_) => return Err(Status::unavailable("internal error")),
	}
	match crate::infrastructure::bridge::is_frozen(&state.pool, user).await {
		Ok(false) => Ok(user),
		Ok(true) => Err(Status::failed_precondition("account is frozen")),
		Err(_) => Err(Status::unavailable("internal error")),
	}
}

/// Gate an RPC on a required money-plane [`Permission`], resolved from the caller's
/// mirrored [`Role`] (the RBAC matrix). Only a human access token qualifies — a service
/// token never carries a user role. The `ADMIN_SUBJECTS` allowlist is purely a **role
/// override** (break-glass bootstrap): when a local user row exists, the disable and
/// revoke gates apply even to an allowlisted subject, so `DisableUser`/`RevokeTokens`
/// bite on the most privileged principals too (mirrors the identity plane's gate). The
/// role is the one the identity plane granted, mirrored onto the local user projection
/// by the bridge; a missing/unknown user is `Investor` (holds nothing), so the gate
/// fails closed. A control-plane read failure fails CLOSED (UNAVAILABLE) — an admin op
/// never proceeds when the gate can't be read.
pub(super) async fn require_permission<T>(state: &AppState, request: &Request<T>, permission: Permission) -> Result<(), Status> {
	let (is_access, sub, token_version) = {
		let claims = claims_of(request).ok_or_else(|| Status::unauthenticated("missing claims"))?;
		(claims.is_access(), claims.sub.clone(), claims.token_version)
	};
	if !is_access {
		return Err(Status::permission_denied("access token required"));
	}
	let allowlisted = state.is_admin(&sub);
	let role = match Uuid::parse_str(&sub) {
		Ok(id) => {
			let id = UserId::from_raw(id);
			let target = state.users.resolve_issuance_by_banking_id(id).await.map_err(|_| Status::unavailable("internal error"))?;
			match target {
				Some(target) => {
					if target.disabled {
						return Err(Status::permission_denied("account is disabled"));
					}
					if token_version < target.token_version {
						return Err(Status::unauthenticated("tokens revoked"));
					}
					if allowlisted {
						Role::Owner
					} else {
						crate::infrastructure::bridge::role_of(&state.pool, id).await.map_err(|_| Status::unavailable("internal error"))?
					}
				}
				// No local row: nothing to gate on — only the break-glass override grants.
				None if allowlisted => Role::Owner,
				None => Role::default(),
			}
		}
		// A non-UUID subject has no user row; only the break-glass override grants.
		Err(_) if allowlisted => Role::Owner,
		Err(_) => return Err(Status::unauthenticated("subject is not a user id")),
	};
	if grants(role, permission) {
		Ok(())
	} else {
		Err(Status::permission_denied("insufficient role"))
	}
}

pub(super) fn parse_user_id(raw: &str) -> Result<UserId, Status> {
	Uuid::parse_str(raw).map(UserId::from_raw).map_err(|_| Status::unauthenticated("subject is not a user id"))
}

pub(super) fn parse_redemption_id(raw: &str) -> Result<RedemptionId, Status> {
	Uuid::parse_str(raw).map(RedemptionId::from_raw).map_err(|_| Status::invalid_argument("invalid redemption id"))
}

pub(super) fn parse_withdrawal_id(raw: &str) -> Result<WithdrawalId, Status> {
	Uuid::parse_str(raw).map(WithdrawalId::from_raw).map_err(|_| Status::invalid_argument("invalid withdrawal id"))
}

/// Treat an empty proto string field as an absent optional.
pub(super) fn optional(raw: &str) -> Option<&str> {
	if raw.is_empty() { None } else { Some(raw) }
}

pub(super) fn unix_now() -> i64 {
	SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or_default()
}

/// Map a domain error to a gRPC status without leaking control-plane internals.
pub(super) fn map_err(err: DomainError) -> Status {
	match err {
		DomainError::NotFound { .. } => Status::not_found(err.to_string()),
		DomainError::Validation(_) => Status::invalid_argument(err.to_string()),
		DomainError::Forbidden(_) => Status::permission_denied(err.to_string()),
		DomainError::Conflict(_) => Status::already_exists(err.to_string()),
		DomainError::Repository(_) => Status::unavailable("internal error"),
	}
}
