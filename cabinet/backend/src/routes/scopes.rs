//! Who may open a vertical's panel: the `allocation:<service_id>` scopes.
//!
//! Identity plane only. Panel access moves no money, so the grants live in concierge's
//! directory and never cross the bridge; the money plane is not asked and no banking token
//! is minted. The BFF deliberately gates on the session alone (`require_identity`, not
//! `require_admin`): a scope's own admin usually holds no global role, and concierge is the
//! one that decides who may read or change a scope — its verdict is forwarded as is.

use axum::{
	Json,
	body::Bytes,
	extract::{Path, State},
	http::HeaderMap,
};
use axum_extra::extract::cookie::CookieJar;
use evconcierge_contracts::concierge::v1 as cc;
use serde_json::{Value, json};

use crate::{
	dto,
	error::ApiError,
	routes::{editable, parse_body, require_identity, required, verify_csrf},
	state::AppState,
};

/// The roles a scope grant may carry. `viewer` is gone (concierge#99): a read-only holder
/// is an ordinary user and gets no grant. Refused here too, so a stale console answers a
/// 400 that names the allowed roles instead of relaying the directory's parse error.
const SCOPE_ROLES: &[&str] = &["operator", "admin"];

/// `allocation:<service_id>`, with `service_id` held to concierge's `[a-z0-9_]{1,64}`.
/// Checked before the call so a path the directory would refuse never costs a round trip,
/// and so nothing but a well-formed scope is ever spliced into one.
fn allocation_scope(service_id: &str) -> Result<String, ApiError> {
	let well_formed = (1..=64).contains(&service_id.len()) && service_id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
	if !well_formed {
		return Err(ApiError::BadRequest("service_id must match [a-z0-9_]{1,64}".into()));
	}
	Ok(format!("allocation:{service_id}"))
}

/// Whom a grant or revocation names. Exactly one of `email` / `user_id`: accepting both
/// would leave the BFF to pick one silently, and concierge answers the two differently
/// for a scope admin (see `GrantScopeRequest.target`).
enum Target {
	UserId(String),
	Email(String),
}

fn target(v: &Value) -> Result<Target, ApiError> {
	match (required(v, "user_id"), required(v, "email")) {
		(Some(user_id), None) => Ok(Target::UserId(user_id)),
		(None, Some(email)) => Ok(Target::Email(email)),
		(Some(_), Some(_)) => Err(ApiError::BadRequest("name the user by email or by user_id, not both".into())),
		(None, None) => Err(ApiError::BadRequest("email or user_id is required".into())),
	}
}

fn scope_role(v: &Value) -> Result<String, ApiError> {
	match required(v, "role") {
		Some(role) if SCOPE_ROLES.contains(&role.as_str()) => Ok(role),
		_ => Err(ApiError::BadRequest("role must be operator or admin".into())),
	}
}

/// `GET /api/admin/allocations/{service_id}/scopes` — the scope's holders, oldest grant
/// first. `legal_name` is empty for a caller who is only the scope's admin.
pub async fn list(State(st): State<AppState>, jar: CookieJar, Path(service_id): Path<String>) -> Result<Json<dto::ScopeHolderList>, ApiError> {
	let (token, _claims) = require_identity(&st, &jar).await?;
	let scope = allocation_scope(&service_id)?;
	Ok(Json(st.grpc.list_scoped_grants(&token, scope).await?.into()))
}

/// `POST /api/admin/allocations/{service_id}/scopes` — `{ email | user_id, role, reason? }`.
/// Replaces the role the user holds on the scope, if any; echoes the grant now in effect.
pub async fn grant(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, Path(service_id): Path<String>, body: Bytes) -> Result<Json<dto::ScopedGrant>, ApiError> {
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let (token, _claims) = require_identity(&st, &jar).await?;
	let scope = allocation_scope(&service_id)?;
	let v = parse_body(&body);
	let req = cc::GrantScopeRequest {
		target: Some(match target(&v)? {
			Target::UserId(id) => cc::grant_scope_request::Target::UserId(id),
			Target::Email(email) => cc::grant_scope_request::Target::Email(email),
		}),
		scope,
		role: scope_role(&v)?,
		reason: editable(&v, "reason"),
	};
	let grant = st.grpc.grant_scope(&token, req).await?.grant.ok_or_else(|| ApiError::Internal("request failed".into()))?;
	Ok(Json(grant.into()))
}

/// `DELETE /api/admin/allocations/{service_id}/scopes` — `{ email | user_id, reason? }`.
/// NOT_FOUND (404) when the user holds no active grant on the scope.
pub async fn revoke(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, Path(service_id): Path<String>, body: Bytes) -> Result<Json<Value>, ApiError> {
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let (token, _claims) = require_identity(&st, &jar).await?;
	let scope = allocation_scope(&service_id)?;
	let v = parse_body(&body);
	let req = cc::RevokeScopeRequest {
		target: Some(match target(&v)? {
			Target::UserId(id) => cc::revoke_scope_request::Target::UserId(id),
			Target::Email(email) => cc::revoke_scope_request::Target::Email(email),
		}),
		scope,
		reason: editable(&v, "reason"),
	};
	st.grpc.revoke_scope(&token, req).await?;
	Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn service_id_follows_the_directory_rule() {
		for ok in ["service_arb", "real_estate", "a", "abc123", &"x".repeat(64)] {
			assert_eq!(allocation_scope(ok).ok(), Some(format!("allocation:{ok}")), "{ok}");
		}
		for bad in ["", "quy-nhon", "Service_Arb", "a b", "a:b", "../x", "é", &"x".repeat(65)] {
			assert!(allocation_scope(bad).is_err(), "{bad:?} must be refused");
		}
	}

	#[test]
	fn viewer_is_not_a_scope_role() {
		assert!(scope_role(&json!({ "role": "viewer" })).is_err());
		assert!(scope_role(&json!({ "role": "owner" })).is_err());
		assert!(scope_role(&json!({})).is_err());
		assert_eq!(scope_role(&json!({ "role": "operator" })).ok().as_deref(), Some("operator"));
		assert_eq!(scope_role(&json!({ "role": "admin" })).ok().as_deref(), Some("admin"));
	}

	#[test]
	fn exactly_one_target() {
		assert!(matches!(target(&json!({ "email": "a@b.c" })), Ok(Target::Email(e)) if e == "a@b.c"));
		assert!(matches!(target(&json!({ "user_id": "u-1" })), Ok(Target::UserId(id)) if id == "u-1"));
		assert!(target(&json!({ "email": "a@b.c", "user_id": "u-1" })).is_err());
		assert!(target(&json!({ "email": "" })).is_err());
		assert!(target(&json!({})).is_err());
	}
}
