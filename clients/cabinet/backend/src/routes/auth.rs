use axum::{
	Json,
	extract::{Query, State},
	http::HeaderMap,
	response::Redirect,
};
use axum_extra::extract::cookie::CookieJar;
use evconcierge_contracts::concierge::v1 as cc;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
	dto::SessionInfo,
	error::ApiError,
	oauth::{Challenge, OAUTH_TX_TTL, authorize_url, safe_return_to},
	routes::{session_id, verify_csrf},
	state::AppState,
};

#[derive(Deserialize)]
pub struct LoginQuery {
	#[serde(rename = "returnTo")]
	return_to: Option<String>,
}

#[derive(Deserialize)]
pub struct CallbackQuery {
	code: Option<String>,
	state: Option<String>,
	error: Option<String>,
}

/// `GET /api/auth/login` — mint PKCE/state/nonce, stash the transaction server-side, and
/// redirect the browser to Google's consent screen.
pub async fn login(State(st): State<AppState>, jar: CookieJar, Query(q): Query<LoginQuery>) -> Result<(CookieJar, Redirect), ApiError> {
	let client_id = st.config.google_client_id.clone().ok_or(ApiError::NotConfigured)?;
	let return_to = safe_return_to(q.return_to.as_deref());
	let ch = Challenge::new();
	let tx_id = st.oauth.put(ch.state.clone(), ch.nonce.clone(), ch.code_verifier.clone(), return_to).await;
	let url = authorize_url(&client_id, &st.config.auth_redirect_uri, &ch.state, &ch.nonce, &ch.code_challenge);
	let jar = jar.add(st.cookies.server_cookie(st.cookies.oauth_tx.clone(), tx_id, OAUTH_TX_TTL));
	Ok((jar, Redirect::to(&url)))
}
/// `GET /api/auth/callback` — validate the state against the stored transaction, exchange
/// the code for concierge's tokens (server-to-server), open a session, and redirect home.
pub async fn callback(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, Query(q): Query<CallbackQuery>) -> (CookieJar, Redirect) {
	if q.error.is_some() {
		return fail(&st, jar, "denied");
	}
	// The transaction is keyed by the HttpOnly `ev_oauth_tx` cookie, so only the browser
	// that started the flow holds it; `state` must then match the stored tx.
	let tx = match jar.get(&st.cookies.oauth_tx).map(|c| c.value().to_string()) {
		Some(id) => st.oauth.take(&id).await,
		None => None,
	};
	let (Some(code), Some(state_param), Some(tx)) = (q.code, q.state, tx) else {
		return fail(&st, jar, "invalid");
	};
	if tx.state != state_param {
		return fail(&st, jar, "invalid");
	}

	let req = cc::ExchangeRequest {
		auth_code: code,
		code_verifier: tx.code_verifier,
		redirect_uri: st.config.auth_redirect_uri.clone(),
		nonce: tx.nonce,
		user_agent: headers.get("user-agent").and_then(|v| v.to_str().ok()).unwrap_or("").to_string(),
		ip: client_ip(&headers),
	};
	match st.grpc.exchange(req).await {
		Ok(tokens) => {
			let (id, csrf, max_age) = st.sessions.put(tokens).await;
			let jar = jar
				.add(st.cookies.server_cookie(st.cookies.session.clone(), id, max_age))
				.add(st.cookies.readable_cookie(st.cookies.csrf.clone(), csrf, max_age));
			let jar = clear_tx(&st, jar);
			(jar, Redirect::to(&safe_return_to(Some(&tx.return_to))))
		}
		Err(e) => {
			// Surface the upstream gRPC status server-side; the user only sees `?error=exchange`.
			tracing::error!(code = ?e.code(), detail = %e.message(), "auth/callback token exchange failed");
			fail(&st, jar, "exchange")
		}
	}
}
/// `GET /api/auth/session` — who-am-I for the browser, refreshing the access token
/// transparently. Never returns a token.
pub async fn session(State(st): State<AppState>, jar: CookieJar) -> (CookieJar, Json<SessionInfo>) {
	let fresh = match session_id(&st, &jar) {
		Some(id) => st.sessions.ensure_fresh(&id, &st.grpc).await,
		None => None,
	};
	match fresh {
		Some((user, _csrf)) => (jar, Json(SessionInfo::authenticated(user))),
		// The session is gone but the browser may still hold the cookie — clear it so the
		// middleware stops treating the request as signed-in (avoids a redirect ping-pong).
		None => (clear_session(&st, jar), Json(SessionInfo::anonymous())),
	}
}
/// `POST /api/auth/logout` — CSRF-checked: drop the session, revoke the refresh family
/// upstream (best-effort), and clear the cookies.
pub async fn logout(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap) -> Result<(CookieJar, Json<Value>), ApiError> {
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	if let Some(id) = session_id(&st, &jar)
		&& let Some(refresh) = st.sessions.forget(&id).await
	{
		// The session is already gone locally; an upstream blip must not block logout.
		let _ = st.grpc.logout(&refresh, false).await;
	}
	Ok((clear_session(&st, jar), Json(json!({ "ok": true }))))
}
/// Clear the OAuth transaction cookie.
fn clear_tx(state: &AppState, jar: CookieJar) -> CookieJar {
	jar.add(state.cookies.removal(state.cookies.oauth_tx.clone(), true))
}

/// Clear the session + csrf cookies (sign-out / dead session).
fn clear_session(state: &AppState, jar: CookieJar) -> CookieJar {
	jar.add(state.cookies.removal(state.cookies.session.clone(), true))
		.add(state.cookies.removal(state.cookies.csrf.clone(), false))
}

/// Abort the callback: clear the tx cookie and bounce to `/login?error=…`.
fn fail(state: &AppState, jar: CookieJar, reason: &str) -> (CookieJar, Redirect) {
	(clear_tx(state, jar), Redirect::to(&format!("/login?error={reason}")))
}

/// Best-effort client IP for the device metadata stored on the refresh-token family.
fn client_ip(headers: &HeaderMap) -> String {
	if let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
		let first = xff.split(',').next().unwrap_or("").trim();
		if !first.is_empty() {
			return first.to_string();
		}
	}
	headers.get("x-real-ip").and_then(|v| v.to_str().ok()).unwrap_or("").to_string()
}
