pub mod auth;
pub mod identity;
pub mod money;
pub mod system;

use axum::{
	Router,
	body::Bytes,
	http::HeaderMap,
	routing::{get, post},
};
use axum_extra::extract::cookie::CookieJar;
use serde_json::Value;
use tower_http::trace::TraceLayer;

use crate::{error::ApiError, state::AppState};

/// Mount every BFF endpoint. Paths and methods mirror the old Next.js route handlers
/// 1:1 so the frontend's same-origin `/api/*` calls are unchanged.
pub fn router(state: AppState) -> Router {
	Router::new()
		.route("/api/health", get(system::health))
		.route("/api/mfe-registry", get(system::mfe_registry))
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
		.route("/api/funds/nav", get(money::fund_nav))
		.route("/api/funds/positions", get(money::list_positions))
		.route("/api/funds/redemptions", get(money::list_redemptions))
		.route("/api/funds/redemptions/cancel", post(money::cancel_redemption))
		.route("/api/funds/subscribe", post(money::subscribe))
		.route("/api/funds/redeem", post(money::redeem))
		.with_state(state)
		.layer(TraceLayer::new_for_http())
}

/// The opaque session id from the request's session cookie.
pub fn session_id(state: &AppState, jar: &CookieJar) -> Option<String> {
	jar.get(&state.cookies.session).map(|c| c.value().to_string())
}

/// The fresh access token for an authenticated request, or `Unauthenticated`.
pub async fn require_token(state: &AppState, jar: &CookieJar) -> Result<String, ApiError> {
	let id = session_id(state, jar).ok_or(ApiError::Unauthenticated)?;
	state.sessions.access_token(&id, &state.grpc).await.ok_or(ApiError::Unauthenticated)
}

/// CSRF double-submit: the `x-ev-csrf` header must equal the readable `ev_csrf` cookie.
pub fn verify_csrf(state: &AppState, jar: &CookieJar, headers: &HeaderMap) -> bool {
	let cookie = jar.get(&state.cookies.csrf).map(|c| c.value().to_string());
	let header = headers.get("x-ev-csrf").and_then(|v| v.to_str().ok());
	matches!((cookie, header), (Some(c), Some(h)) if !c.is_empty() && c == h)
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
