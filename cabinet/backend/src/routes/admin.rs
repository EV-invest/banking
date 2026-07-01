//! Admin-console routes — the operator surface behind `/api/admin/*`.
//!
//! Every handler is coarse-gated by [`require_admin`] (a non-investor session) and then
//! forwards the correct plane token — the concierge identity token for identity/platform
//! RPCs, the banking money token for money/treasury RPCs — which the owning plane
//! re-checks against the specific permission (defense in depth; an insufficient role
//! surfaces as 403). Mutations verify CSRF first, exactly like the money routes.

use axum::{
	Json,
	body::Bytes,
	extract::{Query, State},
	http::HeaderMap,
};
use axum_extra::extract::cookie::CookieJar;
use evbanking_contracts::banking::v1 as bk;
use evconcierge_contracts::concierge::v1 as cc;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
	dto,
	error::ApiError,
	routes::{editable, parse_body, require_admin, require_money_token, require_token, required, verify_csrf},
	state::AppState,
};

#[derive(Deserialize)]
pub struct UserIdQuery {
	user_id: Option<String>,
}

#[derive(Deserialize)]
pub struct ListUsersQuery {
	query: Option<String>,
	role: Option<String>,
	status: Option<String>,
	limit: Option<u32>,
	offset: Option<u32>,
}

fn bool_field(v: &Value, key: &str) -> bool {
	v.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn u32_field(v: &Value, key: &str) -> u32 {
	v.get(key).and_then(Value::as_u64).unwrap_or(0) as u32
}

// ── overview (fleet health; health RPCs are public — no token) ─────────────────

/// `GET /api/admin/overview` — fleet health across the two hubs + the money plane's
/// readiness diagnostics. The frontend composes the remaining rows (microservices,
/// redis, Sentry, PostHog) against the shared observability libs.
pub async fn overview(State(st): State<AppState>, jar: CookieJar) -> Result<Json<dto::AdminOverview>, ApiError> {
	require_admin(&st, &jar).await?;

	let mut services = Vec::new();
	let core = st.grpc.check().await;
	let core_ok = core.is_ok();
	services.push(fleet("piggybank · core", "hub", core_ok, core.map(|c| c.status).unwrap_or_else(|_| "unreachable".into())));
	// Auth runs in-process with core, so it shares core's liveness.
	services.push(fleet("piggybank · auth", "hub", core_ok, if core_ok { "ok".into() } else { "unreachable".into() }));

	let readiness = st.grpc.readiness().await.ok();
	if let Some(r) = &readiness {
		services.push(fleet("postgres", "datastore", r.db_ok, if r.db_ok { "ok".into() } else { "unreachable".into() }));
		services.push(fleet("tigerbeetle", "datastore", r.ledger_ok, if r.ledger_ok { "ok".into() } else { "unreachable".into() }));
	}

	let concierge = st.grpc.concierge_check().await;
	services.push(fleet("concierge", "hub", concierge.is_ok(), concierge.map(|c| c.status).unwrap_or_else(|_| "unreachable".into())));

	Ok(Json(dto::AdminOverview {
		services,
		parked_rows: readiness.as_ref().map(|r| r.parked_rows.to_string()).unwrap_or_else(|| "0".into()),
		backlog: readiness.as_ref().map(|r| r.backlog.to_string()).unwrap_or_else(|| "0".into()),
		oldest_backlog_age_secs: readiness.as_ref().map(|r| r.oldest_backlog_age_secs.to_string()).unwrap_or_else(|| "0".into()),
	}))
}

fn fleet(name: &str, kind: &str, healthy: bool, detail: String) -> dto::FleetService {
	dto::FleetService {
		name: name.into(),
		kind: kind.into(),
		status: if healthy { "healthy".into() } else { "degraded".into() },
		detail,
	}
}

// ── users (concierge identity plane) ───────────────────────────────────────────

/// `GET /api/admin/users` — paginated/filtered user list.
pub async fn list_users(State(st): State<AppState>, jar: CookieJar, Query(q): Query<ListUsersQuery>) -> Result<Json<dto::AdminUserList>, ApiError> {
	require_admin(&st, &jar).await?;
	let token = require_token(&st, &jar).await?;
	let req = cc::ListUsersRequest {
		query: q.query.unwrap_or_default(),
		role: q.role.unwrap_or_default(),
		status: q.status.unwrap_or_default(),
		limit: q.limit.unwrap_or(0),
		offset: q.offset.unwrap_or(0),
	};
	let list = st.grpc.admin_list_users(&token, req).await.map_err(|s| ApiError::read(s, "users unavailable"))?;
	Ok(Json(list.into()))
}

/// `GET /api/admin/users/detail?user_id=` — any user's full profile.
pub async fn get_user(State(st): State<AppState>, jar: CookieJar, Query(q): Query<UserIdQuery>) -> Result<Json<dto::UserProfile>, ApiError> {
	require_admin(&st, &jar).await?;
	let token = require_token(&st, &jar).await?;
	let user_id = q.user_id.unwrap_or_default();
	let profile = st.grpc.admin_get_user(&token, &user_id).await.map_err(|s| ApiError::read(s, "user unavailable"))?;
	Ok(Json(profile.into()))
}

/// `POST /api/admin/users/role` — grant a role (Owner-only at the plane).
pub async fn set_role(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<Value>, ApiError> {
	require_admin(&st, &jar).await?;
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_token(&st, &jar).await?;
	let v = parse_body(&body);
	let (Some(user_id), Some(role)) = (required(&v, "user_id"), required(&v, "role")) else {
		return Err(ApiError::BadRequest("user_id and role are required".into()));
	};
	let res = st.grpc.admin_set_role(&token, &user_id, &role).await?;
	Ok(Json(json!({ "role": res.role })))
}

/// `POST /api/admin/users/suspend` — disable a user.
pub async fn suspend_user(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<Value>, ApiError> {
	require_admin(&st, &jar).await?;
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_token(&st, &jar).await?;
	let Some(user_id) = required(&parse_body(&body), "user_id") else {
		return Err(ApiError::BadRequest("user_id is required".into()));
	};
	st.grpc.admin_disable_user(&token, &user_id).await?;
	Ok(Json(json!({ "ok": true })))
}

/// `POST /api/admin/users/reinstate` — re-enable a disabled user.
pub async fn reinstate_user(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<Value>, ApiError> {
	require_admin(&st, &jar).await?;
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_token(&st, &jar).await?;
	let Some(user_id) = required(&parse_body(&body), "user_id") else {
		return Err(ApiError::BadRequest("user_id is required".into()));
	};
	st.grpc.admin_reinstate_user(&token, &user_id).await?;
	Ok(Json(json!({ "ok": true })))
}

/// `POST /api/admin/users/revoke` — revoke all of a user's sessions (bump token_version).
pub async fn revoke_sessions(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<Value>, ApiError> {
	require_admin(&st, &jar).await?;
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_token(&st, &jar).await?;
	let Some(user_id) = required(&parse_body(&body), "user_id") else {
		return Err(ApiError::BadRequest("user_id is required".into()));
	};
	let res = st.grpc.admin_revoke_tokens(&token, &user_id).await?;
	Ok(Json(json!({ "token_version": res.token_version.to_string() })))
}

/// `POST /api/admin/users/kyc` — set a user's KYC level.
pub async fn set_kyc(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<Value>, ApiError> {
	require_admin(&st, &jar).await?;
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_token(&st, &jar).await?;
	let v = parse_body(&body);
	let Some(user_id) = required(&v, "user_id") else {
		return Err(ApiError::BadRequest("user_id is required".into()));
	};
	let res = st.grpc.admin_set_kyc_level(&token, &user_id, u32_field(&v, "kyc_level")).await?;
	Ok(Json(json!({ "kyc_level": res.kyc_level })))
}

/// `GET /api/admin/users/balance?user_id=` — any user's live balance (money plane).
pub async fn user_balance(State(st): State<AppState>, jar: CookieJar, Query(q): Query<UserIdQuery>) -> Result<Json<dto::UserBalance>, ApiError> {
	require_admin(&st, &jar).await?;
	let token = require_money_token(&st, &jar).await?;
	let user_id = q.user_id.unwrap_or_default();
	let balance = st.grpc.admin_user_balance(&token, &user_id).await.map_err(|s| ApiError::read(s, "balance unavailable"))?;
	Ok(Json(balance.into()))
}

// ── treasury + valuation (banking money plane) ─────────────────────────────────

/// `GET /api/admin/treasury` — the two-layer chart of accounts.
pub async fn treasury(State(st): State<AppState>, jar: CookieJar) -> Result<Json<dto::Treasury>, ApiError> {
	require_admin(&st, &jar).await?;
	let token = require_money_token(&st, &jar).await?;
	let treasury = st.grpc.treasury(&token).await.map_err(|s| ApiError::read(s, "treasury unavailable"))?;
	Ok(Json(treasury.into()))
}

/// `GET /api/admin/valuation/queue` — the cross-user redemption queue awaiting settle.
pub async fn redemption_queue(State(st): State<AppState>, jar: CookieJar) -> Result<Json<dto::RedemptionQueue>, ApiError> {
	require_admin(&st, &jar).await?;
	let token = require_money_token(&st, &jar).await?;
	let queue = st.grpc.redemption_queue(&token).await.map_err(|s| ApiError::read(s, "redemption queue unavailable"))?;
	Ok(Json(queue.into()))
}

/// `POST /api/admin/valuation/post` — post a fund NAV (with the fat-finger guard).
pub async fn post_valuation(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<dto::FundNav>, ApiError> {
	require_admin(&st, &jar).await?;
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_money_token(&st, &jar).await?;
	let v = parse_body(&body);
	let (Some(service), Some(aum)) = (required(&v, "service"), required(&v, "aum")) else {
		return Err(ApiError::BadRequest("service and aum are required".into()));
	};
	let req = bk::PostFundValuationRequest {
		service,
		aum,
		r#override: bool_field(&v, "override"),
	};
	Ok(Json(st.grpc.post_valuation(&token, req).await?.into()))
}

/// `POST /api/admin/valuation/settle` — settle a queued redemption.
pub async fn settle_redemption(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<dto::Redemption>, ApiError> {
	require_admin(&st, &jar).await?;
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_money_token(&st, &jar).await?;
	let Some(id) = required(&parse_body(&body), "redemption_id") else {
		return Err(ApiError::BadRequest("redemption_id is required".into()));
	};
	Ok(Json(st.grpc.settle_redemption(&token, &id).await?.into()))
}

/// `POST /api/admin/valuation/fail` — fail (void + refund units) a queued redemption.
pub async fn fail_redemption(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<dto::Redemption>, ApiError> {
	require_admin(&st, &jar).await?;
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_money_token(&st, &jar).await?;
	let Some(id) = required(&parse_body(&body), "redemption_id") else {
		return Err(ApiError::BadRequest("redemption_id is required".into()));
	};
	Ok(Json(st.grpc.fail_redemption(&token, &id).await?.into()))
}

// ── cabinet (concierge platform config + banking read-only kill-switch) ─────────

/// `GET /api/admin/cabinet` — platform config (concierge) + the money-plane read-only
/// flag (banking, best-effort). The MFE registry is served separately at
/// `/api/mfe-registry`.
pub async fn cabinet_config(State(st): State<AppState>, jar: CookieJar) -> Result<Json<Value>, ApiError> {
	require_admin(&st, &jar).await?;
	let token = require_token(&st, &jar).await?;
	let config: dto::PlatformConfig = st.grpc.platform_config(&token).await.map_err(|s| ApiError::read(s, "platform config unavailable"))?.into();
	// The read-only kill-switch lives on the money plane; fetch best-effort so the screen
	// still renders if the money token isn't available.
	let read_only = match require_money_token(&st, &jar).await {
		Ok(mt) => st.grpc.operations_mode(&mt).await.map(|m| m.read_only).unwrap_or(false),
		Err(_) => false,
	};
	Ok(Json(json!({ "platform": config, "read_only": read_only })))
}

/// `POST /api/admin/cabinet/maintenance` — toggle the cabinet maintenance holding page.
pub async fn set_maintenance(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<dto::PlatformConfig>, ApiError> {
	require_admin(&st, &jar).await?;
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_token(&st, &jar).await?;
	let config = st.grpc.set_maintenance_mode(&token, bool_field(&parse_body(&body), "enabled")).await?;
	Ok(Json(config.into()))
}

/// `POST /api/admin/cabinet/read-only` — toggle the money-plane read-only kill-switch.
pub async fn set_read_only(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<dto::OperationsMode>, ApiError> {
	require_admin(&st, &jar).await?;
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_money_token(&st, &jar).await?;
	let mode = st.grpc.set_operations_mode(&token, bool_field(&parse_body(&body), "read_only")).await?;
	Ok(Json(mode.into()))
}

/// `POST /api/admin/cabinet/announcement` — set/clear the live announcement banner.
pub async fn set_announcement(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<dto::PlatformConfig>, ApiError> {
	require_admin(&st, &jar).await?;
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_token(&st, &jar).await?;
	let v = parse_body(&body);
	let req = cc::SetAnnouncementRequest {
		title: editable(&v, "title"),
		body: editable(&v, "body"),
		active: bool_field(&v, "active"),
	};
	let config = st.grpc.set_announcement(&token, req).await?;
	Ok(Json(config.into()))
}

/// `POST /api/admin/cabinet/flag` — upsert a feature flag.
pub async fn set_flag(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<dto::PlatformConfig>, ApiError> {
	require_admin(&st, &jar).await?;
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_token(&st, &jar).await?;
	let v = parse_body(&body);
	let Some(key) = required(&v, "key") else {
		return Err(ApiError::BadRequest("key is required".into()));
	};
	let req = cc::SetFeatureFlagRequest {
		key,
		description: editable(&v, "description"),
		enabled: bool_field(&v, "enabled"),
		rollout: u32_field(&v, "rollout"),
	};
	let config = st.grpc.set_feature_flag(&token, req).await?;
	Ok(Json(config.into()))
}
