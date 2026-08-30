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
// These are the five calls that make it operable. Paying the collected revenue OUT is
// not among them: `/api/admin/revenue/payout` already does that, debiting the same `fee`
// claim through the ordinary withdrawal pipeline.

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

/// The five `/api/admin/fees/*` routes, driven end to end through the real router.
///
/// These are the calls the operator's fees screen makes, and they were the one part of
/// the fee plane with no automated coverage: the hub's own charging is exercised against
/// Postgres + TigerBeetle in `piggybank/core/tests/fee_policy.rs`, but everything between
/// the browser and that hub — the admin gate, the CSRF double-submit, the "reject before
/// any upstream call" validation, and the proto→JSON mapping the screen reads — was only
/// ever tried by hand against a stub.
///
/// So the test stands up the whole seam in process: a tonic server that plays all three
/// upstreams (the concierge JWKS + directory, the banking token-issuance seam, and the
/// piggybank fees service), a real `AppState` pointed at it, and the real
/// [`router`](crate::routes::router). Nothing below the HTTP boundary is faked out, so a
/// handler that forwards the wrong field, forgets a gate, or renames a JSON key fails
/// here rather than on the screen.
#[cfg(test)]
mod fee_route_tests {
	// `Status` is a large error type tonic mandates in handler signatures.
	#![allow(clippy::result_large_err)]

	use std::{
		net::{SocketAddr, TcpListener},
		sync::{Arc, Mutex},
		time::Duration,
	};

	use axum::{
		Router,
		body::Body,
		http::{Request, StatusCode, header},
	};
	use evbanking_contracts::banking::v1::{
		auth_service_server::{AuthService as BkAuthService, AuthServiceServer as BkAuthServiceServer},
		fees_service_server::{FeesService, FeesServiceServer},
	};
	use evconcierge_auth::{Claims, TokenType, Verifier, VerifierConfig};
	use evconcierge_contracts::concierge::v1::{
		auth_service_server::{AuthService as CcAuthService, AuthServiceServer as CcAuthServiceServer},
		user_directory_server::{UserDirectory, UserDirectoryServer},
	};
	use jsonwebtoken::{Algorithm, EncodingKey, Header, encode, get_current_timestamp};
	use serde_json::Value;
	use tonic::{Code, Request as GrpcRequest, Response as GrpcResponse, Status, transport::Server};
	use tower::ServiceExt;

	use super::*;
	use crate::{
		config::AppConfig,
		cookies::CookieNames,
		routes::router,
		session::BankingTokens,
		state::{AppState, Grpc},
	};

	/// A throwaway Ed25519 keypair (`openssl genpkey -algorithm ed25519`) — the same one
	/// the concierge verifier's own tests use. It signs nothing outside this file.
	const TEST_PEM: &str = "-----BEGIN PRIVATE KEY-----\nMC4CAQAwBQYDK2VwBCIEIKolOSMXwE+tafZkX+jkKYJbmJ066f4E12wAwTIkKps6\n-----END PRIVATE KEY-----\n";
	const TEST_JWK_X: &str = "Z6BCmq9-_wo9d7co5CDW84Wn0sAC3BA0XWK2AOstpV4";
	const TEST_KID: &str = "test-kid";
	const ISSUER: &str = "https://auth.test";
	const AUDIENCE: &str = "concierge";
	const CSRF: &str = "csrf-token-value";
	const SERVICE: &str = "quy-nhon";

	// ── the stub hub ────────────────────────────────────────────────────────────

	/// What the BFF actually forwarded upstream, so a test can assert on the request the
	/// hub would have seen rather than only on the response the browser gets.
	#[derive(Default)]
	struct Seen {
		set_policy: Option<bk::SetFeePolicyRequest>,
		settle: Option<bk::SettleFeeSharesRequest>,
		money_tokens_issued: usize,
	}

	#[derive(Clone)]
	struct Hub {
		/// The role `GetMe` reports — what [`require_admin`] gates on.
		role: String,
		/// When set, every fees RPC fails with this code (the upstream-refusal cases).
		fail_with: Option<Code>,
		seen: Arc<Mutex<Seen>>,
	}

	impl Hub {
		fn new(role: &str) -> Self {
			Self {
				role: role.to_string(),
				fail_with: None,
				seen: Arc::new(Mutex::new(Seen::default())),
			}
		}

		fn failing(role: &str, code: Code) -> Self {
			Self {
				fail_with: Some(code),
				..Self::new(role)
			}
		}

		fn guard(&self) -> Result<(), Status> {
			match self.fail_with {
				Some(code) => Err(Status::new(code, "upstream refused")),
				None => Ok(()),
			}
		}
	}

	#[tonic::async_trait]
	impl CcAuthService for Hub {
		/// The only concierge auth RPC the BFF reaches: the verifier caches these keys and
		/// checks the `ev_access` cookie against them locally.
		async fn jwks(&self, _: GrpcRequest<cc::JwksRequest>) -> Result<GrpcResponse<cc::JwksResponse>, Status> {
			Ok(GrpcResponse::new(cc::JwksResponse {
				keys: vec![cc::Jwk {
					kid: TEST_KID.into(),
					kty: "OKP".into(),
					crv: "Ed25519".into(),
					x: TEST_JWK_X.into(),
					alg: "EdDSA".into(),
					r#use: "sig".into(),
				}],
			}))
		}

		async fn exchange(&self, _: GrpcRequest<cc::ExchangeRequest>) -> Result<GrpcResponse<cc::TokenResponse>, Status> {
			Err(Status::unimplemented("not reached by the fees routes"))
		}

		async fn refresh(&self, _: GrpcRequest<cc::RefreshRequest>) -> Result<GrpcResponse<cc::TokenResponse>, Status> {
			Err(Status::unimplemented("not reached by the fees routes"))
		}

		async fn logout(&self, _: GrpcRequest<cc::LogoutRequest>) -> Result<GrpcResponse<cc::LogoutResponse>, Status> {
			Err(Status::unimplemented("not reached by the fees routes"))
		}

		async fn list_sessions(&self, _: GrpcRequest<cc::ListSessionsRequest>) -> Result<GrpcResponse<cc::ListSessionsResponse>, Status> {
			Err(Status::unimplemented("not reached by the fees routes"))
		}

		async fn revoke_session(&self, _: GrpcRequest<cc::RevokeSessionRequest>) -> Result<GrpcResponse<cc::RevokeSessionResponse>, Status> {
			Err(Status::unimplemented("not reached by the fees routes"))
		}
	}

	#[tonic::async_trait]
	impl UserDirectory for Hub {
		/// `require_admin` reads the caller's role from here per request — the JWT stays
		/// role-free on purpose, so this is the only thing standing between an investor
		/// and the console.
		async fn get_me(&self, _: GrpcRequest<cc::GetMeRequest>) -> Result<GrpcResponse<cc::UserProfile>, Status> {
			Ok(GrpcResponse::new(cc::UserProfile {
				user_id: "user-1".into(),
				role: self.role.clone(),
				..Default::default()
			}))
		}

		async fn update_profile(&self, _: GrpcRequest<cc::UpdateProfileRequest>) -> Result<GrpcResponse<cc::UserProfile>, Status> {
			Err(Status::unimplemented("not reached by the fees routes"))
		}

		async fn revoke_tokens(&self, _: GrpcRequest<cc::RevokeTokensRequest>) -> Result<GrpcResponse<cc::RevokeTokensResponse>, Status> {
			Err(Status::unimplemented("not reached by the fees routes"))
		}

		async fn disable_user(&self, _: GrpcRequest<cc::DisableUserRequest>) -> Result<GrpcResponse<cc::DisableUserResponse>, Status> {
			Err(Status::unimplemented("not reached by the fees routes"))
		}

		async fn reinstate_user(&self, _: GrpcRequest<cc::ReinstateUserRequest>) -> Result<GrpcResponse<cc::ReinstateUserResponse>, Status> {
			Err(Status::unimplemented("not reached by the fees routes"))
		}

		async fn set_kyc_level(&self, _: GrpcRequest<cc::SetKycLevelRequest>) -> Result<GrpcResponse<cc::SetKycLevelResponse>, Status> {
			Err(Status::unimplemented("not reached by the fees routes"))
		}

		async fn list_users(&self, _: GrpcRequest<cc::ListUsersRequest>) -> Result<GrpcResponse<cc::ListUsersResponse>, Status> {
			Err(Status::unimplemented("not reached by the fees routes"))
		}

		async fn get_user(&self, _: GrpcRequest<cc::GetUserRequest>) -> Result<GrpcResponse<cc::UserProfile>, Status> {
			Err(Status::unimplemented("not reached by the fees routes"))
		}

		async fn set_role(&self, _: GrpcRequest<cc::SetRoleRequest>) -> Result<GrpcResponse<cc::SetRoleResponse>, Status> {
			Err(Status::unimplemented("not reached by the fees routes"))
		}
	}

	#[tonic::async_trait]
	impl BkAuthService for Hub {
		/// The concierge→banking exchange seam. Every fees route needs a money token, so
		/// counting the mints is also how these tests prove a rejected request never got
		/// far enough to ask for one.
		async fn issue_user_token(&self, _: GrpcRequest<bk::IssueUserTokenRequest>) -> Result<GrpcResponse<bk::TokenResponse>, Status> {
			self.seen.lock().unwrap().money_tokens_issued += 1;
			Ok(GrpcResponse::new(bk::TokenResponse {
				access_token: "banking-access-token".into(),
				access_expires_at: now_secs() + 900,
				refresh_token: "banking-refresh-token".into(),
				refresh_expires_at: now_secs() + 86_400,
				user: None,
			}))
		}

		async fn refresh(&self, _: GrpcRequest<bk::RefreshRequest>) -> Result<GrpcResponse<bk::TokenResponse>, Status> {
			Err(Status::unimplemented("not reached by the fees routes"))
		}

		async fn logout(&self, _: GrpcRequest<bk::LogoutRequest>) -> Result<GrpcResponse<bk::LogoutResponse>, Status> {
			Err(Status::unimplemented("not reached by the fees routes"))
		}

		async fn list_sessions(&self, _: GrpcRequest<bk::ListSessionsRequest>) -> Result<GrpcResponse<bk::ListSessionsResponse>, Status> {
			Err(Status::unimplemented("not reached by the fees routes"))
		}

		async fn revoke_session(&self, _: GrpcRequest<bk::RevokeSessionRequest>) -> Result<GrpcResponse<bk::RevokeSessionResponse>, Status> {
			Err(Status::unimplemented("not reached by the fees routes"))
		}

		async fn jwks(&self, _: GrpcRequest<bk::JwksRequest>) -> Result<GrpcResponse<bk::JwksResponse>, Status> {
			Err(Status::unimplemented("not reached by the fees routes"))
		}
	}

	#[tonic::async_trait]
	impl FeesService for Hub {
		async fn list_fee_policies(&self, _: GrpcRequest<bk::ListFeePoliciesRequest>) -> Result<GrpcResponse<bk::FeePolicyList>, Status> {
			self.guard()?;
			Ok(GrpcResponse::new(bk::FeePolicyList {
				policies: vec![bk::FeePolicy {
					service: SERVICE.into(),
					configured: true,
					management_bps: 200,
					performance_bps: 2_000,
					hurdle_bps: 500,
					basis: "invested_capital".into(),
					crystallization: "annual".into(),
					updated_at: 1_750_000_000,
				}],
			}))
		}

		async fn set_fee_policy(&self, request: GrpcRequest<bk::SetFeePolicyRequest>) -> Result<GrpcResponse<bk::FeePolicy>, Status> {
			self.guard()?;
			let req = request.into_inner();
			self.seen.lock().unwrap().set_policy = Some(req.clone());
			Ok(GrpcResponse::new(bk::FeePolicy {
				service: req.service,
				configured: true,
				management_bps: req.management_bps,
				performance_bps: req.performance_bps,
				hurdle_bps: req.hurdle_bps,
				basis: req.basis,
				crystallization: req.crystallization,
				updated_at: 1_750_000_100,
			}))
		}

		async fn get_fee_shares(&self, _: GrpcRequest<bk::GetFeeSharesRequest>) -> Result<GrpcResponse<bk::FeeShares>, Status> {
			self.guard()?;
			Ok(GrpcResponse::new(bk::FeeShares {
				service: SERVICE.into(),
				units: "12.500000".into(),
				value: "1375.00".into(),
			}))
		}

		async fn settle_fee_shares(&self, request: GrpcRequest<bk::SettleFeeSharesRequest>) -> Result<GrpcResponse<bk::FeeSettlement>, Status> {
			self.guard()?;
			let req = request.into_inner();
			self.seen.lock().unwrap().settle = Some(req.clone());
			Ok(GrpcResponse::new(bk::FeeSettlement {
				service: req.service,
				units: "12.500000".into(),
				nav: "110.00".into(),
				cash: "1375.00".into(),
			}))
		}

		async fn list_fund_fee_assessments(&self, _: GrpcRequest<bk::ListFundFeeAssessmentsRequest>) -> Result<GrpcResponse<bk::FeeAssessmentList>, Status> {
			self.guard()?;
			Ok(GrpcResponse::new(bk::FeeAssessmentList {
				assessments: vec![bk::FeeAssessment {
					service: SERVICE.into(),
					trigger: "period".into(),
					nav: "110.00".into(),
					management: "20.00".into(),
					performance: "80.00".into(),
					debt_opening: "0".into(),
					charged_units: "0.909090".into(),
					charged_cash: "100.00".into(),
					debt_carried: "0".into(),
					high_water_mark: "110.00".into(),
					assessed_at: 1_750_000_200,
				}],
			}))
		}

		async fn get_fee_policy(&self, _: GrpcRequest<bk::GetFeePolicyRequest>) -> Result<GrpcResponse<bk::FeePolicy>, Status> {
			Err(Status::unimplemented("not reached by the admin fees routes"))
		}

		async fn list_fee_assessments(&self, _: GrpcRequest<bk::ListFeeAssessmentsRequest>) -> Result<GrpcResponse<bk::FeeAssessmentList>, Status> {
			Err(Status::unimplemented("not reached by the admin fees routes"))
		}

		async fn get_accrued_fees(&self, _: GrpcRequest<bk::GetAccruedFeesRequest>) -> Result<GrpcResponse<bk::AccruedFees>, Status> {
			Err(Status::unimplemented("not reached by the admin fees routes"))
		}
	}

	// ── harness ─────────────────────────────────────────────────────────────────

	fn now_secs() -> i64 {
		std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
	}

	/// Serve the stub on an ephemeral port and wait until it accepts, so the first RPC
	/// does not race the listener.
	async fn serve(hub: Hub) -> SocketAddr {
		// Bind to claim a free port, then hand the address to tonic. `serve_with_incoming`
		// would close the gap, but it needs a stream adapter this workspace does not carry.
		let addr = TcpListener::bind("127.0.0.1:0").expect("claim an ephemeral port").local_addr().unwrap();

		tokio::spawn(async move {
			Server::builder()
				.add_service(CcAuthServiceServer::new(hub.clone()))
				.add_service(UserDirectoryServer::new(hub.clone()))
				.add_service(BkAuthServiceServer::new(hub.clone()))
				.add_service(FeesServiceServer::new(hub))
				.serve(addr)
				.await
				.expect("the stub hub serves");
		});

		for _ in 0..100 {
			if tokio::net::TcpStream::connect(addr).await.is_ok() {
				return addr;
			}
			tokio::time::sleep(Duration::from_millis(20)).await;
		}
		panic!("the stub hub never accepted a connection on {addr}");
	}

	/// The real router over a real `AppState`, with all three upstream channels pointed at
	/// the one stub. Cookies take their insecure (unprefixed) names, as in development.
	fn app(addr: SocketAddr) -> Router {
		let env = |var: &str| -> Option<String> {
			Some(match var {
				"PIGGYBANK_GRPC_ADDR" | "BANKING_AUTH_GRPC_ADDR" | "CONCIERGE_GRPC_ADDR" => format!("http://{addr}"),
				"BANKING_ISSUANCE_TOKEN" => "test-issuance".into(),
				"AUTH_ISSUER" => ISSUER.into(),
				"AUTH_CLIENT_AUDIENCE" => AUDIENCE.into(),
				"MFE_REGISTRY_PATH" => "/mfe-registry.json".into(),
				"APP_ENV" => "development".into(),
				_ => return None,
			})
		};
		let config = AppConfig::from_source(env).expect("the test env loads");
		let verifier = Verifier::try_new(VerifierConfig {
			issuer: ISSUER.into(),
			audiences: vec![AUDIENCE.into()],
			allowed_types: vec![TokenType::Access],
			jwks_grpc_endpoint: format!("http://{addr}"),
		})
		.expect("build the verifier");
		let endpoint = format!("http://{addr}");

		router(AppState {
			cookies: Arc::new(CookieNames::new(config.cookie_secure())),
			banking: Arc::new(BankingTokens::new()),
			verifier,
			grpc: Grpc::connect_lazy(&endpoint, &endpoint, &endpoint, Some("test-issuance".into())).expect("build the lazy channels"),
			config: Arc::new(config),
		})
	}

	/// A valid `ev_access` cookie value — signed with the key the stub publishes.
	fn access_token() -> String {
		let claims = Claims {
			sub: "user-1".into(),
			iss: ISSUER.into(),
			aud: AUDIENCE.into(),
			exp: get_current_timestamp() + 900,
			iat: get_current_timestamp(),
			typ: TokenType::Access,
			jti: None,
			token_version: 0,
		};
		let mut header = Header::new(Algorithm::EdDSA);
		header.kid = Some(TEST_KID.into());
		encode(&header, &claims, &EncodingKey::from_ed_pem(TEST_PEM.as_bytes()).unwrap()).expect("sign the access token")
	}

	/// A request carrying the signed session cookie (and, for mutations, the matching
	/// CSRF pair) — what the browser actually sends.
	fn signed(method: &str, uri: &str, body: Option<&str>, csrf: bool) -> Request<Body> {
		let cookie = format!("ev_access={}; ev_csrf={CSRF}", access_token());
		let mut builder = Request::builder().method(method).uri(uri).header(header::COOKIE, cookie);
		if csrf {
			builder = builder.header("x-ev-csrf", CSRF);
		}
		match body {
			Some(json) => builder.header(header::CONTENT_TYPE, "application/json").body(Body::from(json.to_owned())),
			None => builder.body(Body::empty()),
		}
		.expect("build the request")
	}

	async fn send(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
		let response = app.clone().oneshot(request).await.expect("the router responds");
		let status = response.status();
		let bytes = axum::body::to_bytes(response.into_body(), 1 << 20).await.expect("read the body");
		(status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
	}

	// ── the gates ───────────────────────────────────────────────────────────────

	/// Every fees route is behind the session cookie. Without one nothing is read, nothing
	/// is written, and no money-plane token is ever minted for the caller.
	#[tokio::test]
	async fn no_session_reaches_nothing() {
		let hub = Hub::new("admin");
		let seen = hub.seen.clone();
		let app = app(serve(hub).await);

		for (method, uri, body) in [
			("GET", "/api/admin/fees/policies", None),
			("GET", "/api/admin/fees/shares?service=quy-nhon", None),
			("GET", "/api/admin/fees/assessments?service=quy-nhon", None),
			("POST", "/api/admin/fees/policy", Some("{}")),
			("POST", "/api/admin/fees/settle", Some("{}")),
		] {
			let mut builder = Request::builder().method(method).uri(uri);
			if body.is_some() {
				builder = builder.header(header::CONTENT_TYPE, "application/json");
			}
			let request = builder.body(body.map_or_else(Body::empty, |b: &str| Body::from(b.to_owned()))).unwrap();
			let (status, _) = send(&app, request).await;
			assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri} must refuse an unauthenticated caller");
		}

		let seen = seen.lock().unwrap();
		assert_eq!(seen.money_tokens_issued, 0, "an unauthenticated request must never mint a money-plane token");
		assert!(seen.set_policy.is_none() && seen.settle.is_none(), "an unauthenticated request must never reach the hub");
	}

	/// A plain investor holds a perfectly valid session — the role is what stops them, and
	/// it is read from the directory per request rather than trusted from the JWT.
	#[tokio::test]
	async fn an_investor_is_refused_the_console() {
		let hub = Hub::new("investor");
		let seen = hub.seen.clone();
		let app = app(serve(hub).await);

		let (status, _) = send(&app, signed("GET", "/api/admin/fees/policies", None, false)).await;
		assert_eq!(status, StatusCode::FORBIDDEN, "an investor must not read the operator's terms table");

		let body = r#"{"service":"quy-nhon","management_bps":200,"performance_bps":2000,"hurdle_bps":0,"basis":"invested_capital","crystallization":"annual"}"#;
		let (status, _) = send(&app, signed("POST", "/api/admin/fees/policy", Some(body), true)).await;
		assert_eq!(status, StatusCode::FORBIDDEN, "an investor must not price a fund");

		assert!(seen.lock().unwrap().set_policy.is_none(), "a refused caller must never reach the hub");
	}

	/// The double-submit gate on the two mutations. A valid session is not enough: a
	/// cross-site post carries the cookie but cannot read it to echo the header.
	#[tokio::test]
	async fn a_mutation_without_the_csrf_header_is_refused() {
		let hub = Hub::new("admin");
		let seen = hub.seen.clone();
		let app = app(serve(hub).await);

		let body = r#"{"service":"quy-nhon","management_bps":200,"performance_bps":2000,"hurdle_bps":0,"basis":"invested_capital","crystallization":"annual"}"#;
		let (status, _) = send(&app, signed("POST", "/api/admin/fees/policy", Some(body), false)).await;
		assert_eq!(status, StatusCode::FORBIDDEN, "pricing a fund must require the CSRF echo");

		let (status, _) = send(&app, signed("POST", "/api/admin/fees/settle", Some(r#"{"service":"quy-nhon"}"#), false)).await;
		assert_eq!(status, StatusCode::FORBIDDEN, "settling must require the CSRF echo");

		let seen = seen.lock().unwrap();
		assert!(seen.set_policy.is_none() && seen.settle.is_none(), "a CSRF failure must be decided before the hub is called");
	}

	// ── reading ─────────────────────────────────────────────────────────────────

	/// The terms table the screen opens on. `updated_at` crosses as a STRING even though
	/// the proto carries an int64 — the browser contract says string, and a silent switch
	/// to a JSON number is exactly the drift this pins.
	#[tokio::test]
	async fn the_policies_table_reaches_the_browser_intact() {
		let app = app(serve(Hub::new("admin")).await);

		let (status, body) = send(&app, signed("GET", "/api/admin/fees/policies", None, false)).await;
		assert_eq!(status, StatusCode::OK);

		let policy = &body["policies"][0];
		assert_eq!(policy["service"], SERVICE);
		assert_eq!(policy["configured"], true);
		assert_eq!(policy["management_bps"], 200);
		assert_eq!(policy["performance_bps"], 2_000);
		assert_eq!(policy["hurdle_bps"], 500);
		assert_eq!(policy["basis"], "invested_capital");
		assert_eq!(policy["crystallization"], "annual");
		assert_eq!(policy["updated_at"], "1750000000", "updated_at must serialize as a string, not a number");
	}

	/// Uncollected units and what they are worth — the right-hand card.
	#[tokio::test]
	async fn uncollected_units_carry_their_current_value() {
		let app = app(serve(Hub::new("admin")).await);

		let (status, body) = send(&app, signed("GET", "/api/admin/fees/shares?service=quy-nhon", None, false)).await;
		assert_eq!(status, StatusCode::OK);
		assert_eq!(body["service"], SERVICE);
		assert_eq!(body["units"], "12.500000");
		assert_eq!(body["value"], "1375.00");
	}

	/// The audit trail. `debt_carried` is the column that explains a charge collecting less
	/// than it assessed, so it must survive the mapping.
	#[tokio::test]
	async fn the_charge_history_keeps_every_column() {
		let app = app(serve(Hub::new("admin")).await);

		let (status, body) = send(&app, signed("GET", "/api/admin/fees/assessments?service=quy-nhon", None, false)).await;
		assert_eq!(status, StatusCode::OK);

		let row = &body["assessments"][0];
		assert_eq!(row["trigger"], "period");
		assert_eq!(row["management"], "20.00");
		assert_eq!(row["performance"], "80.00");
		assert_eq!(row["charged_units"], "0.909090");
		assert_eq!(row["debt_carried"], "0");
		assert_eq!(row["high_water_mark"], "110.00");
		assert_eq!(row["assessed_at"], "1750000200", "assessed_at must serialize as a string, not a number");
	}

	/// Both per-fund reads name their fund in the query string, and a missing one is a
	/// client error decided here — never a call to the hub with an empty service id.
	#[tokio::test]
	async fn a_per_fund_read_without_a_fund_is_rejected_locally() {
		let hub = Hub::new("admin");
		let seen = hub.seen.clone();
		let app = app(serve(hub).await);

		for uri in [
			"/api/admin/fees/shares",
			"/api/admin/fees/assessments",
			"/api/admin/fees/shares?service=",
			"/api/admin/fees/assessments?service=%20",
		] {
			let (status, _) = send(&app, signed("GET", uri, None, false)).await;
			assert_eq!(status, StatusCode::BAD_REQUEST, "{uri} must be rejected before the hub is called");
		}

		assert_eq!(seen.lock().unwrap().money_tokens_issued, 0, "a request rejected on its own input must not mint a money token");
	}

	// ── writing ─────────────────────────────────────────────────────────────────

	/// Pricing a fund. Every rate is forwarded as given — the handler must not reorder,
	/// default, or drop a field, because the tuple IS the fee schedule.
	#[tokio::test]
	async fn pricing_a_fund_forwards_the_whole_schedule() {
		let hub = Hub::new("admin");
		let seen = hub.seen.clone();
		let app = app(serve(hub).await);

		let body = r#"{"service":"quy-nhon","management_bps":150,"performance_bps":1500,"hurdle_bps":500,"basis":"market_value","crystallization":"quarterly"}"#;
		let (status, response) = send(&app, signed("POST", "/api/admin/fees/policy", Some(body), true)).await;
		assert_eq!(status, StatusCode::OK);
		assert_eq!(response["management_bps"], 150);
		assert_eq!(response["crystallization"], "quarterly");

		let forwarded = seen.lock().unwrap().set_policy.clone().expect("the hub saw the write");
		assert_eq!(forwarded.service, SERVICE);
		assert_eq!(forwarded.management_bps, 150);
		assert_eq!(forwarded.performance_bps, 1_500);
		assert_eq!(forwarded.hurdle_bps, 500);
		assert_eq!(forwarded.basis, "market_value");
		assert_eq!(forwarded.crystallization, "quarterly");
	}

	/// A fee schedule is read as a whole, so the write is all-or-nothing rather than a
	/// patch: omitting the fund, the basis or the period must fail loudly here, not
	/// silently leave a fund on terms nobody chose.
	#[tokio::test]
	async fn a_partial_schedule_is_refused_before_it_reaches_the_hub() {
		let hub = Hub::new("admin");
		let seen = hub.seen.clone();
		let app = app(serve(hub).await);

		for body in [
			r#"{"management_bps":200,"basis":"invested_capital","crystallization":"annual"}"#,
			r#"{"service":"quy-nhon","management_bps":200,"crystallization":"annual"}"#,
			r#"{"service":"quy-nhon","management_bps":200,"basis":"invested_capital"}"#,
			r#"{"service":"quy-nhon","basis":"","crystallization":"annual"}"#,
		] {
			let (status, _) = send(&app, signed("POST", "/api/admin/fees/policy", Some(body), true)).await;
			assert_eq!(status, StatusCode::BAD_REQUEST, "an incomplete schedule must be refused: {body}");
		}

		assert!(seen.lock().unwrap().set_policy.is_none(), "an incomplete schedule must never reach the hub");
	}

	/// The ordinary end-of-period call the screen's one button makes: an empty `units`.
	/// That must forward an EMPTY units string — the hub's "settle the whole balance"
	/// sentinel — rather than a zero, which would settle nothing.
	#[tokio::test]
	async fn settling_without_an_amount_asks_for_the_whole_balance() {
		let hub = Hub::new("admin");
		let seen = hub.seen.clone();
		let app = app(serve(hub).await);

		let (status, body) = send(&app, signed("POST", "/api/admin/fees/settle", Some(r#"{"service":"quy-nhon","units":""}"#), true)).await;
		assert_eq!(status, StatusCode::OK);
		assert_eq!(body["cash"], "1375.00");
		assert_eq!(body["nav"], "110.00");

		let forwarded = seen.lock().unwrap().settle.clone().expect("the hub saw the settle");
		assert_eq!(forwarded.service, SERVICE);
		assert_eq!(forwarded.units, "", "an omitted amount must settle the whole balance, not zero units");
	}

	/// A partial settlement is a deliberate act, and the amount must survive verbatim —
	/// a decimal string, never a float that could round the manager's draw.
	#[tokio::test]
	async fn a_partial_settlement_forwards_its_exact_amount() {
		let hub = Hub::new("admin");
		let seen = hub.seen.clone();
		let app = app(serve(hub).await);

		let (status, _) = send(&app, signed("POST", "/api/admin/fees/settle", Some(r#"{"service":"quy-nhon","units":"3.250000"}"#), true)).await;
		assert_eq!(status, StatusCode::OK);
		assert_eq!(seen.lock().unwrap().settle.clone().unwrap().units, "3.250000");
	}

	/// Settling without naming a fund is refused locally.
	#[tokio::test]
	async fn settling_must_name_its_fund() {
		let hub = Hub::new("admin");
		let seen = hub.seen.clone();
		let app = app(serve(hub).await);

		let (status, _) = send(&app, signed("POST", "/api/admin/fees/settle", Some(r#"{"units":"1"}"#), true)).await;
		assert_eq!(status, StatusCode::BAD_REQUEST);
		assert!(seen.lock().unwrap().settle.is_none(), "a settle with no fund must never reach the hub");
	}

	// ── what the hub says back ──────────────────────────────────────────────────

	/// The money plane re-checks the specific permission and can refuse a caller this
	/// coarse gate let through. That refusal must reach the browser as a 403 carrying the
	/// hub's own client-safe reason — the settle card shows it verbatim.
	#[tokio::test]
	async fn an_upstream_refusal_on_a_write_surfaces_with_its_reason() {
		let app = app(serve(Hub::failing("admin", Code::PermissionDenied)).await);

		let (status, body) = send(&app, signed("POST", "/api/admin/fees/settle", Some(r#"{"service":"quy-nhon","units":""}"#), true)).await;
		assert_eq!(status, StatusCode::FORBIDDEN, "the money plane's own permission check must surface as 403");
		assert_eq!(body["error"], "upstream refused", "a mutation relays the hub's client-safe detail");
	}

	/// A refused settlement — the fund's claim cannot cover it on top of the queued
	/// redemptions — is a precondition failure, not a queued job.
	#[tokio::test]
	async fn a_settlement_the_fund_cannot_cover_is_a_precondition_failure() {
		let app = app(serve(Hub::failing("admin", Code::FailedPrecondition)).await);

		let (status, _) = send(&app, signed("POST", "/api/admin/fees/settle", Some(r#"{"service":"quy-nhon","units":""}"#), true)).await;
		assert_eq!(status, StatusCode::PRECONDITION_FAILED, "an uncoverable settlement must be refused, never queued");
	}

	/// Reads are the other half of the contract: a failing hub must map its code but
	/// surface the handler's fixed message, so internal detail never rides out on a GET.
	#[tokio::test]
	async fn a_failing_read_maps_the_code_but_hides_the_detail() {
		let app = app(serve(Hub::failing("admin", Code::Internal)).await);

		let (status, body) = send(&app, signed("GET", "/api/admin/fees/policies", None, false)).await;
		assert_eq!(status, StatusCode::BAD_GATEWAY);
		assert_eq!(body["error"], "fee policies unavailable", "a read must not relay the hub's own message");

		let (status, body) = send(&app, signed("GET", "/api/admin/fees/shares?service=quy-nhon", None, false)).await;
		assert_eq!(status, StatusCode::BAD_GATEWAY);
		assert_eq!(body["error"], "fee shares unavailable");

		let (status, body) = send(&app, signed("GET", "/api/admin/fees/assessments?service=quy-nhon", None, false)).await;
		assert_eq!(status, StatusCode::BAD_GATEWAY);
		assert_eq!(body["error"], "fee assessments unavailable");
	}
}
