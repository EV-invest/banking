pub mod admin;
pub mod approval;
pub mod consilium;
pub mod governance_ws;
pub mod identity;
pub mod money;
pub mod notifications;
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
use evconcierge_auth::Claims;
use serde_json::Value;
use subtle::ConstantTimeEq;
use tonic::Status;
use tower_http::{timeout::TimeoutLayer, trace::TraceLayer};

use crate::{error::ApiError, session::MoneyToken, state::AppState};

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
///
/// The governance websocket is merged in AFTER [`REQUEST_DEADLINE`] is applied, so the
/// layer wraps every request-shaped route and none of the long-lived one. A deadline is
/// exactly right for a request that must finish and exactly wrong for a socket that must
/// not: inside it, the live consilium page would be dropped every 15 seconds. (The
/// upstream gRPC channel's own per-RPC timeout does not bound a server-stream either — it
/// covers the response future, which resolves when the headers arrive, not the body.)
pub fn router(state: AppState) -> Router {
	let websocket = Router::new().route("/api/owners/consilium/ws", get(governance_ws::upgrade)).with_state(state.clone());

	requests(state).merge(websocket).layer(TraceLayer::new_for_http())
}

/// Every request-shaped endpoint: served under the outer deadline.
fn requests(state: AppState) -> Router {
	Router::new()
		.route("/api/health", get(system::health))
		.route("/api/mfe-registry", get(system::mfe_registry))
		.route("/api/platform", get(platform::status))
		.route("/api/users", get(identity::get_me).patch(identity::update_profile))
		// Notifications — self-service only: concierge resolves the subscriber from the
		// forwarded user token, so no route here names one.
		.route("/api/notifications", get(notifications::list))
		.route("/api/notifications/unread-count", get(notifications::unread_count))
		.route("/api/notifications/read", post(notifications::mark_read))
		.route("/api/notifications/settings", get(notifications::settings))
		.route("/api/notifications/settings/channel", post(notifications::set_channel))
		.route("/api/notifications/settings/topic", post(notifications::set_topic))
		.route("/api/wallet", get(money::get_wallet))
		.route("/api/wallet/deposit-address", get(money::deposit_address))
		.route("/api/wallet/withdrawals", get(money::list_withdrawals).post(money::request_withdrawal))
		.route("/api/wallet/withdrawals/cancel", post(money::cancel_withdrawal))
		.route("/api/wallet/deposits", get(money::list_deposits))
		.route("/api/allocations", get(money::list_allocations))
		.route("/api/allocations/detail", get(money::get_allocation))
		.route("/api/operations", get(money::list_operations))
		.route("/api/funds/nav", get(money::fund_nav))
		.route("/api/funds/positions", get(money::list_positions))
		.route("/api/funds/fee-policy", get(money::fee_policy))
		.route("/api/funds/accrued-fees", get(money::accrued_fees))
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
		.route("/api/admin/treasury/record-deposit", post(admin::record_treasury_deposit))
		.route("/api/admin/allocations", get(admin::list_allocations))
		.route("/api/admin/allocations/register", post(admin::register_allocation))
		.route("/api/admin/allocations/update", post(admin::update_allocation))
		.route("/api/admin/allocations/state", post(admin::set_allocation_state))
		.route("/api/admin/allocations/cap", post(admin::set_allocation_unit_cap))
		.route("/api/admin/fees/policies", get(admin::list_fee_policies))
		.route("/api/admin/fees/policy", post(admin::set_fee_policy))
		.route("/api/admin/fees/shares", get(admin::fee_shares))
		.route("/api/admin/fees/settle", post(admin::settle_fee_shares))
		.route("/api/admin/fees/assessments", get(admin::fund_fee_assessments))
		.route("/api/admin/valuation/queue", get(admin::redemption_queue))
		.route("/api/admin/valuation/post", post(admin::post_valuation))
		.route("/api/admin/valuation/settle", post(admin::settle_redemption))
		.route("/api/admin/valuation/fail", post(admin::fail_redemption))
		.route("/api/admin/withdrawals/queue", get(admin::withdrawal_queue))
		.route("/api/admin/withdrawals/dispatch", post(admin::dispatch_withdrawal))
		.route("/api/admin/withdrawals/settle", post(admin::settle_withdrawal))
		.route("/api/admin/withdrawals/fail", post(admin::fail_withdrawal))
		.route("/api/admin/revenue", get(admin::fund_revenue))
		.route("/api/admin/revenue/payout", post(admin::request_revenue_payout))
		.route("/api/admin/revenue/cancel", post(admin::cancel_revenue_payout))
		.route("/api/admin/revenue/payouts", get(admin::revenue_payouts))
		.route("/api/admin/outbox/parked", get(admin::parked_events))
		.route("/api/admin/outbox/unpark", post(admin::unpark_event))
		.route("/api/admin/cabinet", get(admin::cabinet_config))
		.route("/api/admin/cabinet/maintenance", post(admin::set_maintenance))
		.route("/api/admin/cabinet/read-only", post(admin::set_read_only))
		.route("/api/admin/cabinet/announcement", post(admin::set_announcement))
		.route("/api/admin/cabinet/flag", post(admin::set_flag))
		// Consilium — the fund's own money leaving, gated on a quorum of owners. Money
		// plane: the tally is computed and verified where the money is.
		.route("/api/consilium", get(consilium::list))
		.route("/api/consilium/revenue-payout", post(consilium::open_revenue_payout))
		.route("/api/consilium/{id}", get(consilium::get))
		.route("/api/consilium/{id}/cancel", post(consilium::cancel))
		// Ownership — seats and their removal. Concierge plane: `Role::Owner` is its fact.
		.route("/api/owners", get(consilium::owners))
		.route("/api/owners/resign", post(consilium::resign))
		.route("/api/owners/removals", get(consilium::list_removals).post(consilium::open_removal))
		.route("/api/owners/removals/{id}/vote", post(consilium::vote_removal))
		.route("/api/owners/removals/{id}/cancel", post(consilium::cancel_removal))
		// The public approval surface. NO session, NO CSRF, and no session cookie is even
		// read: the emailed token in the path is the whole credential. See `approval`.
		.route("/api/approval/payout/{token}", get(approval::payout_invitation).post(approval::payout_decision))
		.route("/api/approval/removal/{token}", get(approval::removal_invitation).post(approval::removal_decision))
		.with_state(state)
		.layer(TimeoutLayer::with_status_code(StatusCode::GATEWAY_TIMEOUT, REQUEST_DEADLINE))
}

/// The verified concierge identity for a request: the shared `ev_access` JWT cookie
/// (set by the shell-owned auth surface) checked locally against the concierge JWKS.
/// Auth is shell-owned — the BFF runs no OAuth and holds no session; this cookie IS
/// the request's credential. Returns the raw token (for forwarding as a bearer) plus
/// its verified claims.
pub async fn require_identity(state: &AppState, jar: &CookieJar) -> Result<(String, Claims), ApiError> {
	let token = jar.get(&state.cookies.access).map(|c| c.value().to_string()).ok_or(ApiError::Unauthenticated)?;
	let claims = state.verifier.verify(&token).await.map_err(|_| ApiError::Unauthenticated)?;
	Ok((token, claims))
}

/// The verified **concierge** identity-plane access token for an authenticated request,
/// or `Unauthenticated`. Money RPCs must NOT use this — see [`require_money_token`].
pub async fn require_token(state: &AppState, jar: &CookieJar) -> Result<String, ApiError> {
	Ok(require_identity(state, jar).await?.0)
}

/// The fresh **banking** (`aud=banking-core`) access token for a money-plane RPC. The two
/// planes are cryptographically separated, so the BFF forwards the banking token here and
/// the concierge token to identity — never one plane's token to the other. The banking pair
/// is minted via the concierge→banking exchange seam (`IssueUserToken`) for the VERIFIED
/// JWT subject — the signature check above is what stops a forged cookie from minting
/// money-plane tokens for an arbitrary id. When none can be obtained (issuance
/// unconfigured, or the bridge hasn't mirrored the user yet) this surfaces
/// `NotConfigured` (503) rather than forwarding the wrong-plane token, which the money
/// verifier would reject on issuer/audience.
pub async fn require_money_token(state: &AppState, jar: &CookieJar) -> Result<String, ApiError> {
	let (_token, claims) = require_identity(state, jar).await?;
	match state.banking.token_for(&claims.sub, &state.grpc).await {
		MoneyToken::Token(token) => Ok(token),
		MoneyToken::NotIssued => Err(ApiError::NotConfigured),
	}
}

/// Coarse admin gate for the console routes: the verified caller must hold a
/// non-investor role. This is defense in depth — the owning plane re-checks the
/// SPECIFIC permission and returns `PermissionDenied` (→ 403) if the role is
/// insufficient for that action; here we only cheaply reject a plain investor before
/// any privileged call. The JWT stays role-free on purpose, so the role comes from the
/// concierge directory per admin request (admin traffic is low; the lookup is one
/// local-plane RPC).
pub async fn require_admin(state: &AppState, jar: &CookieJar) -> Result<(), ApiError> {
	let (token, _claims) = require_identity(state, jar).await?;
	let me = state.grpc.get_me(&token).await.map_err(|_| ApiError::Unauthenticated)?;
	if me.role.is_empty() || me.role == "investor" {
		return Err(ApiError::Grpc(Status::permission_denied("admin access required")));
	}
	Ok(())
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
