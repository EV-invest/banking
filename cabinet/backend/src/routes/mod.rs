pub mod admin;
pub mod auth;
pub mod identity;
pub mod money;
pub mod platform;
pub mod system;

use std::time::Duration;

use axum::{
	Router,
	body::Bytes,
	http::{HeaderMap, StatusCode},
	routing::{get, post},
};
use axum_extra::extract::cookie::CookieJar;
use serde_json::Value;
use subtle::ConstantTimeEq;
use tonic::Status;
use tower_http::{timeout::TimeoutLayer, trace::TraceLayer};

use crate::{
	error::ApiError,
	session::{MoneyToken, User},
	state::AppState,
};

/// Outer per-request deadline: a handler that is still awaiting an upstream plane past
/// this bound is aborted and the response becomes a 504, so a wedged plane can never hold
/// a browser connection (or, via the per-session refresh lock, sibling requests) open
/// indefinitely. Looser than the upstream per-RPC [`REQUEST_TIMEOUT`] so an upstream stall
/// normally surfaces as a gRPC error first; this is the backstop for everything else.
///
/// [`REQUEST_TIMEOUT`]: crate::state
const REQUEST_DEADLINE: Duration = Duration::from_secs(15);

/// Mount every BFF endpoint. Paths and methods mirror the old Next.js route handlers
/// 1:1 so the frontend's same-origin `/api/*` calls are unchanged.
pub fn router(state: AppState) -> Router {
	Router::new()
		.route("/api/health", get(system::health))
		.route("/api/mfe-registry", get(system::mfe_registry))
		.route("/api/platform", get(platform::status))
		.route("/api/auth/login", get(auth::login))
		.route("/api/auth/callback", get(auth::callback))
		.route("/api/auth/session", get(auth::session))
		.route("/api/auth/logout", post(auth::logout))
		.route("/api/users", get(identity::get_me).patch(identity::update_profile))
		.route("/api/sessions", get(identity::list_sessions).delete(identity::revoke_session))
		.route("/api/wallet", get(money::get_wallet))
		.route("/api/wallet/deposit-address", get(money::deposit_address))
		.route("/api/wallet/withdrawals", get(money::list_withdrawals).post(money::request_withdrawal))
		.route("/api/wallet/withdrawals/cancel", post(money::cancel_withdrawal))
		.route("/api/wallet/deposits", get(money::list_deposits))
		.route("/api/funds/nav", get(money::fund_nav))
		.route("/api/funds/positions", get(money::list_positions))
		.route("/api/funds/redemptions", get(money::list_redemptions))
		.route("/api/funds/redemptions/cancel", post(money::cancel_redemption))
		.route("/api/funds/subscribe", post(money::subscribe))
		.route("/api/funds/redeem", post(money::redeem))
		// Admin console — role-gated at the BFF (coarse) AND re-checked per-permission by the
		// owning plane (defense in depth). Identity/platform routes hit concierge; money/
		// treasury routes hit the piggybank money plane.
		.route("/api/admin/overview", get(admin::overview))
		.route("/api/admin/users", get(admin::list_users))
		.route("/api/admin/users/detail", get(admin::get_user))
		.route("/api/admin/users/role", post(admin::set_role))
		.route("/api/admin/users/suspend", post(admin::suspend_user))
		.route("/api/admin/users/reinstate", post(admin::reinstate_user))
		.route("/api/admin/users/revoke", post(admin::revoke_sessions))
		.route("/api/admin/users/kyc", post(admin::set_kyc))
		.route("/api/admin/users/balance", get(admin::user_balance))
		.route("/api/admin/treasury", get(admin::treasury))
		.route("/api/admin/valuation/queue", get(admin::redemption_queue))
		.route("/api/admin/valuation/post", post(admin::post_valuation))
		.route("/api/admin/valuation/settle", post(admin::settle_redemption))
		.route("/api/admin/valuation/fail", post(admin::fail_redemption))
		.route("/api/admin/outbox/parked", get(admin::parked_events))
		.route("/api/admin/outbox/unpark", post(admin::unpark_event))
		.route("/api/admin/cabinet", get(admin::cabinet_config))
		.route("/api/admin/cabinet/maintenance", post(admin::set_maintenance))
		.route("/api/admin/cabinet/read-only", post(admin::set_read_only))
		.route("/api/admin/cabinet/announcement", post(admin::set_announcement))
		.route("/api/admin/cabinet/flag", post(admin::set_flag))
		.with_state(state)
		.layer(TimeoutLayer::with_status_code(StatusCode::GATEWAY_TIMEOUT, REQUEST_DEADLINE))
		.layer(TraceLayer::new_for_http())
}

/// The opaque session id from the request's session cookie.
pub fn session_id(state: &AppState, jar: &CookieJar) -> Option<String> {
	jar.get(&state.cookies.session).map(|c| c.value().to_string())
}

/// The fresh **concierge** identity-plane access token for an authenticated request, or
/// `Unauthenticated`. Money RPCs must NOT use this — see [`require_money_token`].
pub async fn require_token(state: &AppState, jar: &CookieJar) -> Result<String, ApiError> {
	let id = session_id(state, jar).ok_or(ApiError::Unauthenticated)?;
	state.sessions.access_token(&id, &state.grpc).await.ok_or(ApiError::Unauthenticated)
}

/// The fresh **banking** (`aud=banking-core`) access token for a money-plane RPC. The two
/// planes are cryptographically separated, so the BFF forwards the banking token here and
/// the concierge token to identity — never one plane's token to the other. The banking pair
/// is minted via the concierge→banking exchange seam (`IssueUserToken`); when none can be
/// obtained (issuance unconfigured, or the bridge hasn't mirrored the user yet) this
/// surfaces `NotConfigured` (503) rather than forwarding the wrong-plane token, which the
/// money verifier would reject on issuer/audience.
pub async fn require_money_token(state: &AppState, jar: &CookieJar) -> Result<String, ApiError> {
	let id = session_id(state, jar).ok_or(ApiError::Unauthenticated)?;
	match state.sessions.money_token(&id, &state.grpc).await {
		MoneyToken::Token(token) => Ok(token),
		MoneyToken::NotIssued => Err(ApiError::NotConfigured),
		MoneyToken::NoSession => Err(ApiError::Unauthenticated),
	}
}

/// Coarse admin gate for the console routes: the live session must belong to a
/// non-investor role. This is defense in depth — the owning plane re-checks the
/// SPECIFIC permission and returns `PermissionDenied` (→ 403) if the role is
/// insufficient for that action; here we only cheaply reject a plain investor before
/// any upstream call. Returns the admin principal (the handler may branch on the exact
/// role). Role is captured at login into the server-side session, so this needs no
/// upstream round trip.
pub async fn require_admin(state: &AppState, jar: &CookieJar) -> Result<User, ApiError> {
	let id = session_id(state, jar).ok_or(ApiError::Unauthenticated)?;
	let (user, _csrf) = state.sessions.ensure_fresh(&id, &state.grpc).await.ok_or(ApiError::Unauthenticated)?;
	if user.role.is_empty() || user.role == "investor" {
		return Err(ApiError::Grpc(Status::permission_denied("admin access required")));
	}
	Ok(user)
}

/// CSRF double-submit: the `x-ev-csrf` header must equal the readable `ev_csrf` cookie.
pub fn verify_csrf(state: &AppState, jar: &CookieJar, headers: &HeaderMap) -> bool {
	let cookie = jar.get(&state.cookies.csrf).map(|c| c.value().to_string());
	let header = headers.get("x-ev-csrf").and_then(|v| v.to_str().ok());
	matches!((cookie.as_deref(), header), (Some(c), Some(h)) if !c.is_empty() && ct_str_eq(c, h))
}

/// Parse a request body leniently (a malformed/empty body becomes `{}`), matching the
/// old BFF's `req.json().catch(() => ({}))`.
pub fn parse_body(body: &Bytes) -> Value {
	serde_json::from_slice(body).unwrap_or_else(|_| Value::Object(Default::default()))
}
/// A required string field: `None` when missing OR empty (matches the TS `!body?.field`).
pub fn required(v: &Value, key: &str) -> Option<String> {
	v.get(key).and_then(|x| x.as_str()).map(str::to_string).filter(|s| !s.is_empty())
}
/// An editable string field: missing ⇒ `""` (full-replace semantics; empty clears).
pub fn editable(v: &Value, key: &str) -> String {
	v.get(key).and_then(|x| x.as_str()).unwrap_or("").to_string()
}
/// Constant-time string equality (after a length check, which only reveals length) as
/// defense-in-depth, matching the constant-time discipline used for secret comparisons.
fn ct_str_eq(a: &str, b: &str) -> bool {
	a.len() == b.len() && a.as_bytes().ct_eq(b.as_bytes()).into()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn ct_str_eq_matches_plain_equality() {
		assert!(ct_str_eq("a3f9c0d1-token", "a3f9c0d1-token"));
		assert!(!ct_str_eq("a3f9c0d1-token", "a3f9c0d1-toked"));
		assert!(!ct_str_eq("short", "longer-value"));
		assert!(ct_str_eq("", ""));
	}
}
