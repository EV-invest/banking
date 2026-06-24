use std::collections::HashMap;

use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use crate::util::{base64url, now_secs, random_token};

const AUTHORIZE_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const SCOPE: &str = "openid email profile";
/// The OAuth handshake (PKCE/state/nonce) lives at most this long between authorize and callback.
pub const OAUTH_TX_TTL: i64 = 600;

/// A fresh PKCE verifier/challenge plus anti-forgery state and nonce.
pub struct Challenge {
	pub state: String,
	pub nonce: String,
	pub code_verifier: String,
	pub code_challenge: String,
}

impl Challenge {
	pub fn new() -> Self {
		let code_verifier = random_token(32);
		let code_challenge = base64url(&Sha256::digest(code_verifier.as_bytes()));
		Self {
			state: random_token(16),
			nonce: random_token(16),
			code_verifier,
			code_challenge,
		}
	}
}

/// Build the Google authorize URL to redirect the browser to.
pub fn authorize_url(client_id: &str, redirect_uri: &str, state: &str, nonce: &str, code_challenge: &str) -> String {
	let query = form_urlencoded::Serializer::new(String::new())
		.append_pair("client_id", client_id)
		.append_pair("redirect_uri", redirect_uri)
		.append_pair("response_type", "code")
		.append_pair("scope", SCOPE)
		.append_pair("state", state)
		.append_pair("nonce", nonce)
		.append_pair("code_challenge", code_challenge)
		.append_pair("code_challenge_method", "S256")
		.append_pair("access_type", "online")
		.append_pair("prompt", "select_account")
		.finish();
	format!("{AUTHORIZE_ENDPOINT}?{query}")
}

/// Keep a post-login redirect target same-origin to defeat open-redirects (port of `safeReturnTo`).
pub fn safe_return_to(raw: Option<&str>) -> String {
	let Some(raw) = raw else { return "/".to_string() };
	if !raw.starts_with('/') {
		return "/".to_string();
	}
	// Reject protocol-relative ("//evil", "/\evil") and any backslash.
	let second = raw.as_bytes().get(1).copied();
	if second == Some(b'/') || second == Some(b'\\') || raw.contains('\\') {
		return "/".to_string();
	}
	raw.to_string()
}

/// One in-flight OAuth login transaction, bound to the `ev_oauth_tx` cookie.
#[derive(Clone)]
pub struct OAuthTx {
	pub state: String,
	pub nonce: String,
	pub code_verifier: String,
	pub return_to: String,
	created_at: i64,
}

/// The OAuth transaction store. In-process map (single-instance/dev), keyed by the
/// HttpOnly `ev_oauth_tx` cookie so only the browser that started the flow can complete it.
pub struct OAuthTxStore {
	txns: Mutex<HashMap<String, OAuthTx>>,
}

impl OAuthTxStore {
	pub fn new() -> Self {
		Self { txns: Mutex::new(HashMap::new()) }
	}

	/// Store a transaction, returning its id (the `ev_oauth_tx` cookie value).
	pub async fn put(&self, state: String, nonce: String, code_verifier: String, return_to: String) -> String {
		let id = random_token(32);
		let tx = OAuthTx {
			state,
			nonce,
			code_verifier,
			return_to,
			created_at: now_secs(),
		};
		self.txns.lock().await.insert(id.clone(), tx);
		id
	}

	/// Read + consume the transaction for `id`, if present and unexpired.
	pub async fn take(&self, id: &str) -> Option<OAuthTx> {
		let tx = self.txns.lock().await.remove(id)?;
		(now_secs() - tx.created_at <= OAUTH_TX_TTL).then_some(tx)
	}
}
