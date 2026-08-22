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
pub struct FeeServiceQuery {
	service: Option<String>,
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
		deposit_scan: readiness
			.as_ref()
			.map(|r| {
				r.scan_cursors
					.iter()
					.map(|c| dto::DepositScan {
						network: c.network.clone(),
						age_secs: c.age_secs.to_string(),
					})
					.collect()
			})
			.unwrap_or_default(),
		unseal_failures: readiness.as_ref().map(|r| r.unseal_failures.to_string()).unwrap_or_else(|| "0".into()),
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

/// `POST /api/admin/treasury/record-deposit` — write a ledger fact for USDT that arrived
/// on-chain out of band.
///
/// Funding a rail's treasury hot wallet directly moves real USDT while producing no ledger
/// entry at all: the rail's `custody` figure stays put, `fund_capital` understates what the
/// company put in, and the withdrawal dispatch gate (`min(TB rail, on-chain treasury)`) keeps
/// reading zero, so that liquidity cannot be spent. This records the missing fact.
///
/// Idempotent by `tx_ref` — deliberately NOT `SeedCapital`, which has no dedup key and
/// double-credits the fund on a retried click. Pass the real on-chain reference (`txhash:logIndex`
/// on an EVM rail) so a re-submission, and any watcher that later scans the same transfer,
/// collapse onto the same key.
pub async fn record_treasury_deposit(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<Value>, ApiError> {
	require_admin(&st, &jar).await?;
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_money_token(&st, &jar).await?;
	let v = parse_body(&body);
	let (Some(tx_ref), Some(network)) = (required(&v, "tx_ref"), required(&v, "network")) else {
		return Err(ApiError::BadRequest("tx_ref and network are required".into()));
	};
	// Amount is an optional ASSERTION, never an input — the money plane reads the real one off
	// the chain. Forwarded so a typo fails loudly there instead of crediting another transfer.
	let req = bk::RecordDepositRequest {
		tx_ref,
		network,
		expected_amount: required(&v, "expected_amount").unwrap_or_default(),
	};
	let res = st.grpc.record_deposit(&token, req).await?;
	// `false` is a successful no-op, not a failure: the tx_ref was already recorded. The
	// caller needs the difference to tell "credited" from "already credited" — and the
	// amount/party come back from the chain, so the operator sees what was actually booked.
	Ok(Json(json!({
		"recorded": res.recorded,
		"amount": res.amount,
		"party_kind": res.party_kind,
		"party_id": res.party_id,
	})))
}

// ── fees ─────────────────────────────────────────────────────────────────────
//
// The fee plane shipped with no operator surface at all: the sweeper ran hourly and
// nothing could give a fund a policy for it to act on, so no fund ever charged anything.
// These are the six calls that make it operable — configure, watch, collect, pay out.

/// `GET /api/admin/fees/policies` — every fund's fee terms, for the fees table.
pub async fn list_fee_policies(State(st): State<AppState>, jar: CookieJar) -> Result<Json<dto::FeePolicyList>, ApiError> {
	require_admin(&st, &jar).await?;
	let token = require_money_token(&st, &jar).await?;
	let list = st.grpc.fee_policies(&token).await.map_err(|s| ApiError::read(s, "fee policies unavailable"))?;
	Ok(Json(list.into()))
}

/// `POST /api/admin/fees/policy` — write a fund's terms.
///
/// Every rate is required rather than patch-style optional. A fee schedule is read as a
/// whole — "2 and 20 over a 5% hurdle, annual" is one statement — and letting an operator
/// change the management rate while leaving an unseen crystallization period in place is
/// how a fund ends up charging terms nobody chose.
pub async fn set_fee_policy(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<dto::FeePolicy>, ApiError> {
	require_admin(&st, &jar).await?;
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_money_token(&st, &jar).await?;
	let v = parse_body(&body);
	let (Some(service), Some(basis), Some(crystallization)) = (required(&v, "service"), required(&v, "basis"), required(&v, "crystallization")) else {
		return Err(ApiError::BadRequest("service, basis and crystallization are required".into()));
	};
	let req = bk::SetFeePolicyRequest {
		service,
		management_bps: u32_field(&v, "management_bps"),
		performance_bps: u32_field(&v, "performance_bps"),
		hurdle_bps: u32_field(&v, "hurdle_bps"),
		basis,
		crystallization,
	};
	let policy = st.grpc.set_fee_policy(&token, req).await?;
	Ok(Json(policy.into()))
}

/// `GET /api/admin/fees/shares?service=` — uncollected fee units in one fund, and their value.
pub async fn fee_shares(State(st): State<AppState>, jar: CookieJar, Query(q): Query<FeeServiceQuery>) -> Result<Json<dto::FeeShares>, ApiError> {
	require_admin(&st, &jar).await?;
	let Some(service) = q.service.filter(|s| !s.trim().is_empty()) else {
		return Err(ApiError::BadRequest("service is required".into()));
	};
	let token = require_money_token(&st, &jar).await?;
	let shares = st.grpc.fee_shares(&token, &service).await.map_err(|s| ApiError::read(s, "fee shares unavailable"))?;
	Ok(Json(shares.into()))
}

/// `POST /api/admin/fees/settle` — convert a fund's accumulated fee units into cash.
///
/// Omitting `units` settles the whole balance, which is the ordinary end-of-period call.
/// Refused rather than queued when the fund's claim cannot cover it on top of its queued
/// redemptions — the manager is paid last.
pub async fn settle_fee_shares(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<dto::FeeSettlement>, ApiError> {
	require_admin(&st, &jar).await?;
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_money_token(&st, &jar).await?;
	let v = parse_body(&body);
	let Some(service) = required(&v, "service") else {
		return Err(ApiError::BadRequest("service is required".into()));
	};
	let settlement = st.grpc.settle_fee_shares(&token, &service, &required(&v, "units").unwrap_or_default()).await?;
	Ok(Json(settlement.into()))
}

/// `GET /api/admin/fees/assessments?service=` — every charge this fund has made.
pub async fn fund_fee_assessments(State(st): State<AppState>, jar: CookieJar, Query(q): Query<FeeServiceQuery>) -> Result<Json<dto::FeeAssessmentList>, ApiError> {
	require_admin(&st, &jar).await?;
	let Some(service) = q.service.filter(|s| !s.trim().is_empty()) else {
		return Err(ApiError::BadRequest("service is required".into()));
	};
	let token = require_money_token(&st, &jar).await?;
	let list = st
		.grpc
		.fund_fee_assessments(&token, &service)
		.await
		.map_err(|s| ApiError::read(s, "fee assessments unavailable"))?;
	Ok(Json(list.into()))
}

/// `POST /api/admin/fees/payout` — pay retained fee revenue to an account that can withdraw it.
///
/// Not idempotent, and the UI must not retry it blindly: there is no external fact to key
/// on, so a second call is a second payout. The gate is the retained balance itself.
pub async fn pay_fee_revenue(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<Value>, ApiError> {
	require_admin(&st, &jar).await?;
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_money_token(&st, &jar).await?;
	let v = parse_body(&body);
	let (Some(user_id), Some(amount)) = (required(&v, "user_id"), required(&v, "amount")) else {
		return Err(ApiError::BadRequest("user_id and amount are required".into()));
	};
	let res = st.grpc.pay_fee_revenue(&token, &user_id, &amount).await?;
	Ok(Json(json!({ "remaining": res.remaining })))
}

/// `GET /api/admin/valuation/queue` — the cross-user redemption queue awaiting settle.
pub async fn redemption_queue(State(st): State<AppState>, jar: CookieJar) -> Result<Json<dto::RedemptionQueue>, ApiError> {
	require_admin(&st, &jar).await?;
	let token = require_money_token(&st, &jar).await?;
	let queue = st.grpc.redemption_queue(&token).await.map_err(|s| ApiError::read(s, "redemption queue unavailable"))?;
	Ok(Json(queue.into()))
}

/// `GET /api/admin/allocations` — the full catalog, drafts and closed included.
pub async fn list_allocations(State(st): State<AppState>, jar: CookieJar) -> Result<Json<dto::AllocationList>, ApiError> {
	require_admin(&st, &jar).await?;
	let token = require_money_token(&st, &jar).await?;
	let list = st.grpc.list_allocations(&token, true).await.map_err(|s| ApiError::read(s, "allocations unavailable"))?;
	Ok(Json(list.into()))
}

/// `POST /api/admin/allocations/register` — register a new investable product (`draft`).
/// This is the only way a fund comes into existence.
pub async fn register_allocation(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<dto::Allocation>, ApiError> {
	require_admin(&st, &jar).await?;
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_money_token(&st, &jar).await?;
	let v = parse_body(&body);
	let (Some(service), Some(title)) = (required(&v, "service"), required(&v, "title")) else {
		return Err(ApiError::BadRequest("service and title are required".into()));
	};
	let req = bk::RegisterAllocationRequest {
		service,
		title,
		summary: required(&v, "summary").unwrap_or_default(),
	};
	Ok(Json(st.grpc.register_allocation(&token, req).await?.into()))
}

/// `POST /api/admin/allocations/update` — edit an allocation's presentation fields.
pub async fn update_allocation(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<dto::Allocation>, ApiError> {
	require_admin(&st, &jar).await?;
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_money_token(&st, &jar).await?;
	let v = parse_body(&body);
	let (Some(service), Some(title)) = (required(&v, "service"), required(&v, "title")) else {
		return Err(ApiError::BadRequest("service and title are required".into()));
	};
	let req = bk::UpdateAllocationRequest {
		service,
		title,
		summary: required(&v, "summary").unwrap_or_default(),
	};
	Ok(Json(st.grpc.update_allocation(&token, req).await?.into()))
}

/// `POST /api/admin/allocations/state` — open or close an allocation. Closing stops new
/// subscriptions only; redemptions keep working so no investor is trapped.
pub async fn set_allocation_state(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<dto::Allocation>, ApiError> {
	require_admin(&st, &jar).await?;
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_money_token(&st, &jar).await?;
	let v = parse_body(&body);
	let (Some(service), Some(state)) = (required(&v, "service"), required(&v, "state")) else {
		return Err(ApiError::BadRequest("service and state are required".into()));
	};
	let req = bk::SetAllocationStateRequest { service, state };
	Ok(Json(st.grpc.set_allocation_state(&token, req).await?.into()))
}

/// `POST /api/admin/allocations/cap` — resize an allocation's authorised unit supply.
/// Separate from `/update` because this one gates money: the hub refuses a subscription
/// that would carry the issued supply past it.
pub async fn set_allocation_unit_cap(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<dto::Allocation>, ApiError> {
	require_admin(&st, &jar).await?;
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_money_token(&st, &jar).await?;
	let v = parse_body(&body);
	let (Some(service), Some(unit_cap)) = (required(&v, "service"), required(&v, "unit_cap")) else {
		return Err(ApiError::BadRequest("service and unit_cap are required".into()));
	};
	let req = bk::SetAllocationUnitCapRequest { service, unit_cap };
	Ok(Json(st.grpc.set_allocation_unit_cap(&token, req).await?.into()))
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

// ── withdrawals (banking money plane, operator actions) ────────────────────────

/// `GET /api/admin/withdrawals/queue` — cross-user withdrawals awaiting operator
/// action (queued / processing), oldest first.
pub async fn withdrawal_queue(State(st): State<AppState>, jar: CookieJar) -> Result<Json<dto::WithdrawalQueue>, ApiError> {
	require_admin(&st, &jar).await?;
	let token = require_money_token(&st, &jar).await?;
	let queue = st.grpc.withdrawal_queue(&token).await.map_err(|s| ApiError::read(s, "withdrawal queue unavailable"))?;
	Ok(Json(queue.into()))
}

/// `POST /api/admin/withdrawals/dispatch` — dispatch a queued withdrawal whose rail
/// now has liquidity.
pub async fn dispatch_withdrawal(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<Value>, ApiError> {
	require_admin(&st, &jar).await?;
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_money_token(&st, &jar).await?;
	let Some(id) = required(&parse_body(&body), "withdrawal_id") else {
		return Err(ApiError::BadRequest("withdrawal_id is required".into()));
	};
	st.grpc.dispatch_withdrawal(&token, &id).await?;
	Ok(Json(json!({ "ok": true })))
}

/// `POST /api/admin/withdrawals/settle` — settle a processing withdrawal with its
/// mined on-chain tx reference.
pub async fn settle_withdrawal(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<Value>, ApiError> {
	require_admin(&st, &jar).await?;
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_money_token(&st, &jar).await?;
	let v = parse_body(&body);
	let Some(id) = required(&v, "withdrawal_id") else {
		return Err(ApiError::BadRequest("withdrawal_id is required".into()));
	};
	let Some(tx_ref) = required(&v, "tx_ref") else {
		return Err(ApiError::BadRequest("tx_ref is required".into()));
	};
	st.grpc.settle_withdrawal(&token, &id, &tx_ref).await?;
	Ok(Json(json!({ "ok": true })))
}

/// `POST /api/admin/withdrawals/fail` — fail a withdrawal that NEVER reached the
/// chain (voids the reservation, refunding the user). The hub refuses when a
/// broadcast row exists; the reason is an audit note.
pub async fn fail_withdrawal(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<Value>, ApiError> {
	require_admin(&st, &jar).await?;
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_money_token(&st, &jar).await?;
	let v = parse_body(&body);
	let Some(id) = required(&v, "withdrawal_id") else {
		return Err(ApiError::BadRequest("withdrawal_id is required".into()));
	};
	let reason = required(&v, "reason").unwrap_or_default();
	st.grpc.fail_withdrawal(&token, &id, &reason).await?;
	Ok(Json(json!({ "ok": true })))
}

// ── revenue payouts (banking money plane, the fund's own money) ────────────────

/// `GET /api/admin/revenue` — what the fund has earned and may pay itself, plus the
/// rails a payout can ship on. Admin/Owner at the plane (`RevenuePayout`); an Operator
/// who may read the treasury is refused here, since this is the payout surface.
pub async fn fund_revenue(State(st): State<AppState>, jar: CookieJar) -> Result<Json<dto::FundRevenue>, ApiError> {
	require_admin(&st, &jar).await?;
	let token = require_money_token(&st, &jar).await?;
	let revenue = st.grpc.fund_revenue(&token).await.map_err(|s| ApiError::read(s, "fund revenue unavailable"))?;
	Ok(Json(revenue.into()))
}

/// `POST /api/admin/revenue/payout` — send earned revenue to an external wallet.
///
/// The cap is enforced at the money plane against the revenue claim's available
/// balance, not here: this handler must never be the thing standing between company
/// money and client money. Accepted-and-queued like any withdrawal when the rail is
/// short, then dispatched/settled through the usual withdrawal queue.
pub async fn request_revenue_payout(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<dto::Withdrawal>, ApiError> {
	require_admin(&st, &jar).await?;
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_money_token(&st, &jar).await?;
	let v = parse_body(&body);
	let (Some(network), Some(address), Some(amount)) = (required(&v, "network"), required(&v, "address"), required(&v, "amount")) else {
		return Err(ApiError::BadRequest("network, address and amount are required".into()));
	};
	let req = bk::RequestRevenuePayoutRequest { network, address, amount };
	Ok(Json(st.grpc.request_revenue_payout(&token, req).await?.into()))
}

/// `POST /api/admin/revenue/cancel` — cancel a still-queued payout (refund to the
/// revenue claim). The hub refuses a user's withdrawal and refuses once processing.
pub async fn cancel_revenue_payout(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<dto::Withdrawal>, ApiError> {
	require_admin(&st, &jar).await?;
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_money_token(&st, &jar).await?;
	let Some(id) = required(&parse_body(&body), "withdrawal_id") else {
		return Err(ApiError::BadRequest("withdrawal_id is required".into()));
	};
	Ok(Json(st.grpc.cancel_revenue_payout(&token, &id).await?.into()))
}

/// `GET /api/admin/revenue/payouts` — the fund's payout history, newest first.
pub async fn revenue_payouts(State(st): State<AppState>, jar: CookieJar) -> Result<Json<dto::WithdrawalList>, ApiError> {
	require_admin(&st, &jar).await?;
	let token = require_money_token(&st, &jar).await?;
	let list = st.grpc.revenue_payouts(&token).await.map_err(|s| ApiError::read(s, "payout history unavailable"))?;
	Ok(Json(list.into()))
}

// ── outbox (banking money plane) ────────────────────────────────────────────────

/// `GET /api/admin/outbox/parked` — outbox rows the relay parked (needs-intervention).
pub async fn parked_events(State(st): State<AppState>, jar: CookieJar) -> Result<Json<dto::ParkedEventList>, ApiError> {
	require_admin(&st, &jar).await?;
	let token = require_money_token(&st, &jar).await?;
	let list = st.grpc.parked_events(&token).await.map_err(|s| ApiError::read(s, "parked events unavailable"))?;
	Ok(Json(list.into()))
}

/// `POST /api/admin/outbox/unpark` — CSRF-checked: clear a park so the relay re-drives
/// the event (the hub refuses compensated/dispatched rows).
pub async fn unpark_event(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<Value>, ApiError> {
	require_admin(&st, &jar).await?;
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_money_token(&st, &jar).await?;
	let Some(seq) = required(&parse_body(&body), "seq").and_then(|s| s.parse::<i64>().ok()) else {
		return Err(ApiError::BadRequest("seq is required".into()));
	};
	st.grpc.unpark_event(&token, seq).await?;
	Ok(Json(json!({ "ok": true })))
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
