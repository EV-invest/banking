//! What a user holds inside a tenant's namespace (`sa`): the grants a relying party's panel
//! gates on.
//!
//! Identity plane only: the grants live in concierge's directory and never cross the bridge,
//! so no banking token is minted. The BFF gates on the session alone (`require_identity`):
//! a tenant's delegate usually holds no global role, and concierge decides who may read or
//! change a namespace — its verdict is forwarded as is.

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

/// A namespace is one segment of concierge's `[a-z0-9_]+`. Checked before the call so
/// nothing but a well-formed one is ever spliced into a request.
fn namespace(raw: String) -> Result<String, ApiError> {
	if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_') {
		return Err(ApiError::BadRequest("namespace must match [a-z0-9_]+".into()));
	}
	Ok(raw)
}

/// The target must sit in the route's namespace, so the URL and the body cannot name two.
/// Its shape inside the namespace is the catalog's business, and concierge checks it.
fn target(v: &Value, namespace: &str) -> Result<String, ApiError> {
	match required(v, "target") {
		Some(target) if target.strip_prefix(namespace).is_some_and(|rest| rest.len() > 1 && rest.starts_with(':')) => Ok(target),
		_ => Err(ApiError::BadRequest(format!("target must be `{namespace}:<name>`"))),
	}
}

/// Whom a grant or revocation names. Exactly one of `email` / `user_id`: accepting both
/// would leave the BFF to pick one silently, and concierge answers the two differently
/// for a delegate (see `GrantPermissionRequest.subject`).
enum Subject {
	UserId(String),
	Email(String),
}

fn subject(v: &Value) -> Result<Subject, ApiError> {
	match (required(v, "user_id"), required(v, "email")) {
		(Some(user_id), None) => Ok(Subject::UserId(user_id)),
		(None, Some(email)) => Ok(Subject::Email(email)),
		(Some(_), Some(_)) => Err(ApiError::BadRequest("name the user by email or by user_id, not both".into())),
		(None, None) => Err(ApiError::BadRequest("email or user_id is required".into())),
	}
}

/// `GET /api/admin/tenants/{namespace}/grants` — the namespace's active grants, oldest
/// first. Names are empty for a caller who is only a delegate; 403 for anyone else, which
/// is also how the console learns whether the caller manages the namespace at all.
pub async fn list(State(st): State<AppState>, jar: CookieJar, Path(ns): Path<String>) -> Result<Json<dto::GrantHolderList>, ApiError> {
	let (token, _claims) = require_identity(&st, &jar).await?;
	Ok(Json(st.grpc.list_grants(&token, namespace(ns)?).await?.into()))
}

/// `POST /api/admin/tenants/{namespace}/grants` — `{ email | user_id, target, reason? }`.
/// Echoes the grant now in effect.
pub async fn grant(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, Path(ns): Path<String>, body: Bytes) -> Result<Json<dto::PermissionGrant>, ApiError> {
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let (token, _claims) = require_identity(&st, &jar).await?;
	let ns = namespace(ns)?;
	let v = parse_body(&body);
	let req = cc::GrantPermissionRequest {
		subject: Some(match subject(&v)? {
			Subject::UserId(id) => cc::grant_permission_request::Subject::UserId(id),
			Subject::Email(email) => cc::grant_permission_request::Subject::Account(email),
		}),
		target: target(&v, &ns)?,
		reason: editable(&v, "reason"),
	};
	let grant = st.grpc.grant_permission(&token, req).await?.grant.ok_or_else(|| ApiError::Internal("request failed".into()))?;
	Ok(Json(grant.into()))
}

/// `DELETE /api/admin/tenants/{namespace}/grants` — `{ email | user_id, target, reason? }`.
/// NOT_FOUND (404) when the user holds no active grant of that target.
pub async fn revoke(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, Path(ns): Path<String>, body: Bytes) -> Result<Json<Value>, ApiError> {
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let (token, _claims) = require_identity(&st, &jar).await?;
	let ns = namespace(ns)?;
	let v = parse_body(&body);
	let req = cc::RevokePermissionRequest {
		subject: Some(match subject(&v)? {
			Subject::UserId(id) => cc::revoke_permission_request::Subject::UserId(id),
			Subject::Email(email) => cc::revoke_permission_request::Subject::Account(email),
		}),
		target: target(&v, &ns)?,
		reason: editable(&v, "reason"),
	};
	st.grpc.revoke_permission(&token, req).await?;
	Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn namespace_follows_the_directory_rule() {
		for ok in ["sa", "real_estate", "a1"] {
			assert_eq!(namespace(ok.into()).ok().as_deref(), Some(ok), "{ok}");
		}
		for bad in ["", "Sa", "s-a", "sa:x", "../x", "é"] {
			assert!(namespace(bad.into()).is_err(), "{bad:?} must be refused");
		}
	}

	#[test]
	fn target_stays_in_the_routes_namespace() {
		for ok in ["sa:operator", "sa:work:leads:read", "sa:work:*"] {
			assert_eq!(target(&json!({ "target": ok }), "sa").ok().as_deref(), Some(ok), "{ok}");
		}
		for bad in ["", "sa", "sa:", "sab:operator", "bank:payment:open", "operator"] {
			assert!(target(&json!({ "target": bad }), "sa").is_err(), "{bad:?} must be refused");
		}
	}

	#[test]
	fn exactly_one_subject() {
		assert!(matches!(subject(&json!({ "email": "a@b.c" })), Ok(Subject::Email(e)) if e == "a@b.c"));
		assert!(matches!(subject(&json!({ "user_id": "u-1" })), Ok(Subject::UserId(id)) if id == "u-1"));
		assert!(subject(&json!({ "email": "a@b.c", "user_id": "u-1" })).is_err());
		assert!(subject(&json!({ "email": "" })).is_err());
		assert!(subject(&json!({})).is_err());
	}
}
