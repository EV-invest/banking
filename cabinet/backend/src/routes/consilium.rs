//! The signed-in governance surface: `/api/consilium/**` (money) and `/api/owners/**`
//! (ownership).
//!
//! Two planes, two tokens, and the split is the point. Paying the fund's own revenue out
//! is authorized in the MONEY plane, against the owner roster it already mirrors, because
//! `docs/ARCHITECTURE.md` refuses to let a concierge-signed artifact move money — and a
//! consilium verdict is exactly such an artifact. Moving a seat — granting one or taking
//! one away — is authorized in the OWNERSHIP plane, because `Role::Owner` is a
//! concierge-owned fact and the bridge between the planes is one-way. So the money
//! handlers forward the banking token and never the concierge one; the ownership handlers
//! forward the concierge token and never the banking one. Neither plane trusts the other's
//! verdict.

use axum::{
	Json,
	body::Bytes,
	extract::{Path, Query, State},
	http::HeaderMap,
};
use axum_extra::extract::cookie::CookieJar;
use evbanking_contracts::banking::v1 as bk;
use serde::Deserialize;

use crate::{
	dto,
	error::ApiError,
	governance::{AdmissionVote, RemovalVote},
	routes::{parse_body, require_money_token, require_token, required, verify_csrf},
	state::AppState,
};

#[derive(Deserialize)]
pub struct LimitQuery {
	limit: Option<u32>,
}

// ── money plane: the revenue-payout consilium ────────────────────────────────

/// `GET /api/consilium` — the governance history, newest first. An absent `limit` falls
/// through to the plane's default page.
pub async fn list(State(st): State<AppState>, jar: CookieJar, Query(q): Query<LimitQuery>) -> Result<Json<dto::ConsiliumList>, ApiError> {
	let token = require_money_token(&st, &jar).await?;
	let list = st.grpc.list_consilia(&token, q.limit.unwrap_or(0)).await.map_err(|s| ApiError::read(s, "consilia unavailable"))?;
	Ok(Json(list.into()))
}

/// `GET /api/consilium/{id}` — one consilium in full, including per-voter state.
pub async fn get(State(st): State<AppState>, jar: CookieJar, Path(id): Path<String>) -> Result<Json<dto::Consilium>, ApiError> {
	let token = require_money_token(&st, &jar).await?;
	let consilium = st.grpc.get_consilium(&token, &id).await.map_err(|s| ApiError::read(s, "consilium unavailable"))?;
	Ok(Json(consilium.into()))
}

/// `POST /api/consilium/revenue-payout` — CSRF-checked: open an invoice to pay fund
/// revenue out. The plane refuses a second open consilium and a roster too small to reach
/// quorum; the terms are immutable once open, so a correction means cancel and reopen.
pub async fn open_revenue_payout(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<dto::Consilium>, ApiError> {
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_money_token(&st, &jar).await?;
	let v = parse_body(&body);
	let (Some(network), Some(address), Some(amount)) = (required(&v, "network"), required(&v, "address"), required(&v, "amount")) else {
		return Err(ApiError::BadRequest("network, address and amount are required".into()));
	};
	// The memo is shown to every owner in their approval mail and is never interpreted,
	// so an absent one is an empty note rather than a rejected request.
	let terms = bk::RevenuePayoutTerms {
		network,
		address,
		amount,
		memo: v.get("memo").and_then(|m| m.as_str()).unwrap_or_default().to_string(),
	};
	Ok(Json(st.grpc.open_revenue_payout(&token, terms).await?.into()))
}

/// `POST /api/consilium/{id}/cancel` — CSRF-checked: the initiator withdraws their own
/// still-open invoice.
pub async fn cancel(State(st): State<AppState>, jar: CookieJar, Path(id): Path<String>, headers: HeaderMap) -> Result<Json<dto::Consilium>, ApiError> {
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_money_token(&st, &jar).await?;
	Ok(Json(st.grpc.cancel_consilium(&token, &id).await?.into()))
}

// ── ownership plane: the roster, its removals and its admissions ─────────────

/// `GET /api/owners` — the current roster, with the below-three-owners warning that says
/// the fund can no longer authorize a payout.
pub async fn owners(State(st): State<AppState>, jar: CookieJar) -> Result<Json<dto::OwnerList>, ApiError> {
	let token = require_token(&st, &jar).await?;
	let owners = st.grpc.list_owners(&token).await.map_err(|s| ApiError::read(s, "owners unavailable"))?;
	Ok(Json(owners.into()))
}

/// `POST /api/owners/resign` — CSRF-checked: give up your own seat. No consilium, but the
/// floor still applies, and `confirm_email` must be the caller's own address — resigning
/// cannot be a stray click.
pub async fn resign(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<dto::OwnerList>, ApiError> {
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_token(&st, &jar).await?;
	let Some(confirm_email) = required(&parse_body(&body), "confirm_email") else {
		return Err(ApiError::BadRequest("confirm_email is required".into()));
	};
	Ok(Json(st.grpc.resign_ownership(&token, &confirm_email).await?.into()))
}

/// `GET /api/owners/removals` — every proposal, open and closed. Nothing is deleted: a
/// rejected, expired or void removal stays readable.
pub async fn list_removals(State(st): State<AppState>, jar: CookieJar, Query(q): Query<LimitQuery>) -> Result<Json<dto::OwnerRemovalList>, ApiError> {
	let token = require_token(&st, &jar).await?;
	let list = st
		.grpc
		.list_owner_removals(&token, q.limit.unwrap_or(0))
		.await
		.map_err(|s| ApiError::read(s, "removals unavailable"))?;
	Ok(Json(list.into()))
}

/// `POST /api/owners/removals` — CSRF-checked: propose removing an owner. The reason is
/// required here as well as upstream: it is shown to the target and to every peer, and a
/// seat taken away without a stated reason is not a record anyone can audit later.
pub async fn open_removal(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<dto::OwnerRemoval>, ApiError> {
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_token(&st, &jar).await?;
	let v = parse_body(&body);
	let (Some(target_user_id), Some(reason)) = (required(&v, "target_user_id"), required(&v, "reason")) else {
		return Err(ApiError::BadRequest("target_user_id and reason are required".into()));
	};
	Ok(Json(st.grpc.open_owner_removal(&token, &target_user_id, &reason).await?.into()))
}

/// `POST /api/owners/removals/{id}/vote` — CSRF-checked: a peer's live vote, cast in the
/// cabinet rather than from a mailbox. The plane refuses the target and the initiator.
pub async fn vote_removal(State(st): State<AppState>, jar: CookieJar, Path(id): Path<String>, headers: HeaderMap, body: Bytes) -> Result<Json<dto::OwnerRemoval>, ApiError> {
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_token(&st, &jar).await?;
	let Some(vote) = required(&parse_body(&body), "vote").as_deref().and_then(RemovalVote::parse) else {
		return Err(ApiError::BadRequest("vote must be \"remove\" or \"keep\"".into()));
	};
	Ok(Json(st.grpc.submit_peer_vote(&token, &id, vote.wire()).await?.into()))
}

/// `POST /api/owners/removals/{id}/cancel` — CSRF-checked: withdraw a proposal the caller
/// opened, while it is still open.
pub async fn cancel_removal(State(st): State<AppState>, jar: CookieJar, Path(id): Path<String>, headers: HeaderMap) -> Result<Json<dto::OwnerRemoval>, ApiError> {
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_token(&st, &jar).await?;
	Ok(Json(st.grpc.cancel_owner_removal(&token, &id).await?.into()))
}

/// `GET /api/owners/admissions` — every admission, open and closed.
pub async fn list_admissions(State(st): State<AppState>, jar: CookieJar, Query(q): Query<LimitQuery>) -> Result<Json<dto::OwnerAdmissionList>, ApiError> {
	let token = require_token(&st, &jar).await?;
	let list = st
		.grpc
		.list_owner_admissions(&token, q.limit.unwrap_or(0))
		.await
		.map_err(|s| ApiError::read(s, "admissions unavailable"))?;
	Ok(Json(list.into()))
}

/// `POST /api/owners/admissions` — CSRF-checked: propose granting a seat. This is the ONLY
/// way `Role::Owner` is granted — the directory's `SetRole` refuses it outside an executed
/// admission — and it passes only on unanimity of every other owner, so a minority can
/// never grow itself into the majority a payout needs.
pub async fn open_admission(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<dto::OwnerAdmission>, ApiError> {
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_token(&st, &jar).await?;
	let v = parse_body(&body);
	let (Some(candidate_user_id), Some(reason)) = (required(&v, "candidate_user_id"), required(&v, "reason")) else {
		return Err(ApiError::BadRequest("candidate_user_id and reason are required".into()));
	};
	Ok(Json(st.grpc.open_owner_admission(&token, &candidate_user_id, &reason).await?.into()))
}

/// `POST /api/owners/admissions/{id}/vote` — CSRF-checked: a peer's vote on an admission.
/// Its own vocabulary, not the removal one: "admit"/"reject" is a different question from
/// "remove"/"keep", and a page that submitted the wrong verb here would seat someone.
pub async fn vote_admission(State(st): State<AppState>, jar: CookieJar, Path(id): Path<String>, headers: HeaderMap, body: Bytes) -> Result<Json<dto::OwnerAdmission>, ApiError> {
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_token(&st, &jar).await?;
	let Some(vote) = required(&parse_body(&body), "vote").as_deref().and_then(AdmissionVote::parse) else {
		return Err(ApiError::BadRequest("vote must be \"admit\" or \"reject\"".into()));
	};
	Ok(Json(st.grpc.submit_admission_vote(&token, &id, vote.wire()).await?.into()))
}

/// `POST /api/owners/admissions/{id}/cancel` — CSRF-checked: withdraw an admission the
/// caller opened, while it is still open.
pub async fn cancel_admission(State(st): State<AppState>, jar: CookieJar, Path(id): Path<String>, headers: HeaderMap) -> Result<Json<dto::OwnerAdmission>, ApiError> {
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_token(&st, &jar).await?;
	Ok(Json(st.grpc.cancel_owner_admission(&token, &id).await?.into()))
}

#[cfg(test)]
mod route_tests {
	use std::{collections::HashMap, sync::Arc};

	use axum::{
		Router,
		body::Body,
		http::{Request, StatusCode, header},
	};
	use evconcierge_auth::{TokenType, Verifier, VerifierConfig};
	use tower::ServiceExt;

	use crate::{
		config::AppConfig,
		cookies::CookieNames,
		routes::{approval::AttemptLimiter, router},
		session::BankingTokens,
		state::{AppState, Grpc},
	};

	/// A port nothing listens on: every one of these tests asserts on a decision the BFF
	/// makes BEFORE it reaches a plane, so the plane only has to be unreachable — and an
	/// unreachable one is exactly what proves a request got past the gates.
	const BLACK_HOLE: &str = "http://127.0.0.1:1";

	fn app() -> Router {
		let env = HashMap::from([
			("PIGGYBANK_GRPC_ADDR", BLACK_HOLE),
			("BANKING_AUTH_GRPC_ADDR", BLACK_HOLE),
			("CONCIERGE_GRPC_ADDR", BLACK_HOLE),
			("BANKING_ISSUANCE_TOKEN", "test-issuance"),
			("AUTH_ISSUER", "https://auth.test"),
			("AUTH_CLIENT_AUDIENCE", "concierge"),
			("MFE_REGISTRY_PATH", "/mfe-registry.json"),
			("APP_ENV", "development"),
		]);
		let config = AppConfig::from_source(|var| env.get(var).map(|value| (*value).to_string())).expect("the test env loads");
		let verifier = Verifier::try_new(VerifierConfig {
			issuer: config.auth_issuer.clone(),
			audiences: vec![config.auth_client_audience.clone()],
			allowed_types: vec![TokenType::Access],
			jwks_grpc_endpoint: BLACK_HOLE.into(),
		})
		.expect("build the verifier");

		router(AppState {
			cookies: Arc::new(CookieNames::new(config.cookie_secure())),
			banking: Arc::new(BankingTokens::new()),
			approvals: Arc::new(AttemptLimiter::default()),
			verifier,
			grpc: Grpc::connect_lazy(BLACK_HOLE, BLACK_HOLE, BLACK_HOLE, Some("test-issuance".into())).expect("build the lazy channels"),
			config: Arc::new(config),
		})
	}

	async fn send(method: &str, uri: &str, body: Option<&str>) -> (StatusCode, HashMap<String, String>) {
		let request = Request::builder().method(method).uri(uri);
		let request = match body {
			Some(json) => request.header(header::CONTENT_TYPE, "application/json").body(Body::from(json.to_owned())),
			None => request.body(Body::empty()),
		}
		.expect("build the request");
		let response = app().oneshot(request).await.expect("the router responds");
		let headers = response
			.headers()
			.iter()
			.map(|(name, value)| (name.as_str().to_string(), value.to_str().unwrap_or_default().to_string()))
			.collect();
		(response.status(), headers)
	}

	/// The governance surface is owner-only, so an anonymous caller is refused before any
	/// plane is touched — and in particular before a money-plane token could be minted.
	#[tokio::test]
	async fn the_signed_in_surface_refuses_an_anonymous_caller() {
		let reads = [
			("GET", "/api/consilium"),
			("GET", "/api/consilium/c-1"),
			("GET", "/api/owners"),
			("GET", "/api/owners/removals"),
			("GET", "/api/owners/admissions"),
		];
		for (method, uri) in reads {
			let (status, _) = send(method, uri, None).await;
			assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri}");
		}
	}

	/// Every mutation on the signed-in surface goes through the double-submit check, and
	/// it runs FIRST — a request without the header is refused as CSRF, not as a missing
	/// session, which is what proves the check is not somewhere further down the handler.
	#[tokio::test]
	async fn every_signed_in_mutation_is_csrf_gated() {
		let mutations = [
			("/api/consilium/revenue-payout", r#"{"network":"TRC20","address":"T1","amount":"10"}"#),
			("/api/consilium/c-1/cancel", "{}"),
			("/api/owners/removals", r#"{"target_user_id":"u-2","reason":"inactive"}"#),
			("/api/owners/removals/r-1/vote", r#"{"vote":"remove"}"#),
			("/api/owners/removals/r-1/cancel", "{}"),
			("/api/owners/admissions", r#"{"candidate_user_id":"u-3","reason":"co-founder"}"#),
			("/api/owners/admissions/a-1/vote", r#"{"vote":"admit"}"#),
			("/api/owners/admissions/a-1/cancel", "{}"),
			("/api/owners/resign", r#"{"confirm_email":"ada@example.com"}"#),
		];
		for (uri, body) in mutations {
			let (status, _) = send("POST", uri, Some(body)).await;
			assert_eq!(status, StatusCode::FORBIDDEN, "POST {uri} must be refused as CSRF");
		}
	}

	/// The emailed surface is the opposite: no session, no CSRF, and it must still reach
	/// the plane. With the plane unreachable that shows up as a transport failure — which
	/// only a request that passed every gate could possibly have produced.
	#[tokio::test]
	async fn the_emailed_surface_needs_no_session_and_no_csrf() {
		let calls = [
			("GET", "/api/approval/payout/tok-1", None),
			("POST", "/api/approval/payout/tok-1", Some(r#"{"code":"ABCDE12345","decision":"approve"}"#)),
		];
		for (method, uri, body) in calls {
			let (status, headers) = send(method, uri, body).await;
			assert_ne!(status, StatusCode::UNAUTHORIZED, "{method} {uri} must not ask for a session");
			assert_ne!(status, StatusCode::FORBIDDEN, "{method} {uri} must not ask for a CSRF token");
			assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{method} {uri} reached the plane and found it down");
			assert_eq!(headers.get("referrer-policy").map(String::as_str), Some("no-referrer"));
			assert_eq!(headers.get("cache-control").map(String::as_str), Some("no-store"));
		}
	}

	/// A vote with no code cannot be spent as one: the malformed body is refused here,
	/// before it can burn one of the token's five attempts upstream.
	#[tokio::test]
	async fn a_vote_without_a_code_never_reaches_the_plane() {
		let (status, _) = send("POST", "/api/approval/payout/tok-1", Some(r#"{"decision":"approve"}"#)).await;
		assert_eq!(status, StatusCode::BAD_REQUEST);
	}
}
