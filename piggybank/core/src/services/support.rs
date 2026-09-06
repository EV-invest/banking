//! Cross-cutting helpers shared by every context service: the auth gates
//! (`caller_id`/`require_permission`), the domain→status error mapper, id parsers, and
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
	money::Network,
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
/// token never carries a user role.
///
/// The role has exactly one source: the persisted `users.role` column the concierge →
/// banking bridge mirrors from the identity plane (`ROLE_CHANGED`). There is no
/// environment-driven override here — the money plane holds no roster of its own, so
/// ownership is whatever concierge persisted and nothing else. Every path fails closed:
/// no local row is [`Role::default`] (holds nothing), a non-UUID subject is
/// `UNAUTHENTICATED`, and a control-plane read failure is `UNAVAILABLE` — an admin op
/// never proceeds when the gate can't be read. The disable and revoke gates run first,
/// so `DisableUser`/`RevokeTokens` bite on the most privileged principals too.
pub(super) async fn require_permission<T>(state: &AppState, request: &Request<T>, permission: Permission) -> Result<(), Status> {
	let (is_access, sub, token_version) = {
		let claims = claims_of(request).ok_or_else(|| Status::unauthenticated("missing claims"))?;
		(claims.is_access(), claims.sub.clone(), claims.token_version)
	};
	if !is_access {
		return Err(Status::permission_denied("access token required"));
	}
	let id = parse_user_id(&sub)?;
	let target = state.users.resolve_issuance_by_banking_id(id).await.map_err(|_| Status::unavailable("internal error"))?;
	let role = match target {
		Some(target) => {
			if target.disabled {
				return Err(Status::permission_denied("account is disabled"));
			}
			if token_version < target.token_version {
				return Err(Status::unauthenticated("tokens revoked"));
			}
			crate::infrastructure::bridge::role_of(&state.pool, id).await.map_err(|_| Status::unavailable("internal error"))?
		}
		// No local row: nothing the mirror can grant, so the caller holds nothing.
		None => Role::default(),
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

/// Whether a rail's addresses are testnet-tagged. Only TON has a distinct testnet address
/// form (the tag is baked into the friendly encoding), so a client can render the raw
/// `workchain:hex` the hub stores in the right user-facing form; the other rails' addresses
/// are network-agnostic on the wire.
pub(super) fn rail_is_testnet(state: &AppState, network: Network) -> bool {
	matches!(network, Network::Ton) && state.ton_is_testnet
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
