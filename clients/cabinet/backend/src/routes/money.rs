use axum::{
	Json,
	body::Bytes,
	extract::{Query, State},
	http::HeaderMap,
};
use axum_extra::extract::cookie::CookieJar;
use evbanking_contracts::banking::v1 as bk;
use serde::Deserialize;

use crate::{
	dto,
	error::ApiError,
	routes::{parse_body, require_money_token, required, verify_csrf},
	state::AppState,
};

#[derive(Deserialize)]
pub struct NetworkQuery {
	network: Option<String>,
}

#[derive(Deserialize)]
pub struct ServiceQuery {
	service: Option<String>,
}

// ── wallet ───────────────────────────────────────────────────────────────────

/// `GET /api/wallet` — the unified lifecycle balance, deposit rails, and per-rail withdraw options.
pub async fn get_wallet(State(st): State<AppState>, jar: CookieJar) -> Result<Json<dto::Wallet>, ApiError> {
	let token = require_money_token(&st, &jar).await?;
	let wallet = st.grpc.get_wallet(&token).await.map_err(|s| ApiError::read(s, "wallet unavailable"))?;
	Ok(Json(wallet.into()))
}

/// `GET /api/wallet/deposit-address?network=` — the caller's deposit address on a network.
pub async fn deposit_address(State(st): State<AppState>, jar: CookieJar, Query(q): Query<NetworkQuery>) -> Result<Json<dto::DepositAddress>, ApiError> {
	let token = require_money_token(&st, &jar).await?;
	let addr = st
		.grpc
		.deposit_address(&token, &q.network.unwrap_or_default())
		.await
		.map_err(|s| ApiError::read(s, "deposit address unavailable"))?;
	Ok(Json(addr.into()))
}

/// `GET /api/wallet/withdrawals` — the caller's withdrawals, newest first.
pub async fn list_withdrawals(State(st): State<AppState>, jar: CookieJar) -> Result<Json<dto::WithdrawalList>, ApiError> {
	let token = require_money_token(&st, &jar).await?;
	let list = st.grpc.list_withdrawals(&token).await.map_err(|s| ApiError::read(s, "withdrawals unavailable"))?;
	Ok(Json(list.into()))
}

/// `POST /api/wallet/withdrawals` — CSRF-checked: open a withdrawal of free balance.
pub async fn request_withdrawal(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<dto::Withdrawal>, ApiError> {
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_money_token(&st, &jar).await?;
	let v = parse_body(&body);
	let (Some(network), Some(address), Some(amount)) = (required(&v, "network"), required(&v, "address"), required(&v, "amount")) else {
		return Err(ApiError::BadRequest("network, address and amount are required".into()));
	};
	let req = bk::RequestWithdrawalRequest { network, address, amount };
	Ok(Json(st.grpc.request_withdrawal(&token, req).await?.into()))
}

/// `POST /api/wallet/withdrawals/cancel` — CSRF-checked: cancel a still-queued withdrawal.
pub async fn cancel_withdrawal(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<dto::Withdrawal>, ApiError> {
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_money_token(&st, &jar).await?;
	let Some(withdrawal_id) = required(&parse_body(&body), "withdrawal_id") else {
		return Err(ApiError::BadRequest("withdrawal_id is required".into()));
	};
	Ok(Json(st.grpc.cancel_withdrawal(&token, &withdrawal_id).await?.into()))
}

// ── funds (the service currency) ─────────────────────────────────────────────

/// `GET /api/funds/nav?service=` — the current NAV (price per share) of a fund.
pub async fn fund_nav(State(st): State<AppState>, jar: CookieJar, Query(q): Query<ServiceQuery>) -> Result<Json<dto::FundNav>, ApiError> {
	let token = require_money_token(&st, &jar).await?;
	let nav = st
		.grpc
		.fund_nav(&token, &q.service.unwrap_or_default())
		.await
		.map_err(|s| ApiError::read(s, "fund nav unavailable"))?;
	Ok(Json(nav.into()))
}

/// `GET /api/funds/positions` — the caller's fund positions.
pub async fn list_positions(State(st): State<AppState>, jar: CookieJar) -> Result<Json<dto::PositionList>, ApiError> {
	let token = require_money_token(&st, &jar).await?;
	let list = st.grpc.list_positions(&token).await.map_err(|s| ApiError::read(s, "positions unavailable"))?;
	Ok(Json(list.into()))
}

/// `GET /api/funds/redemptions` — the caller's redemptions, newest first.
pub async fn list_redemptions(State(st): State<AppState>, jar: CookieJar) -> Result<Json<dto::RedemptionList>, ApiError> {
	let token = require_money_token(&st, &jar).await?;
	let list = st.grpc.list_redemptions(&token).await.map_err(|s| ApiError::read(s, "redemptions unavailable"))?;
	Ok(Json(list.into()))
}

/// `POST /api/funds/subscribe` — CSRF-checked: subscribe free balance into a fund (mint units).
pub async fn subscribe(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<dto::Subscription>, ApiError> {
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_money_token(&st, &jar).await?;
	let v = parse_body(&body);
	let (Some(service), Some(amount)) = (required(&v, "service"), required(&v, "amount")) else {
		return Err(ApiError::BadRequest("service and amount are required".into()));
	};
	let req = bk::SubscribeRequest { service, amount };
	Ok(Json(st.grpc.subscribe(&token, req).await?.into()))
}

/// `POST /api/funds/redeem` — CSRF-checked: redeem units back to cash (accept-and-queue).
pub async fn redeem(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<dto::Redemption>, ApiError> {
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_money_token(&st, &jar).await?;
	let v = parse_body(&body);
	let (Some(service), Some(units)) = (required(&v, "service"), required(&v, "units")) else {
		return Err(ApiError::BadRequest("service and units are required".into()));
	};
	let req = bk::RedeemRequest { service, units };
	Ok(Json(st.grpc.redeem(&token, req).await?.into()))
}

/// `POST /api/funds/redemptions/cancel` — CSRF-checked: cancel a still-queued redemption.
pub async fn cancel_redemption(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<dto::Redemption>, ApiError> {
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_money_token(&st, &jar).await?;
	let Some(redemption_id) = required(&parse_body(&body), "redemption_id") else {
		return Err(ApiError::BadRequest("redemption_id is required".into()));
	};
	Ok(Json(st.grpc.cancel_redemption(&token, &redemption_id).await?.into()))
}
