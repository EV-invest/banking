//! Google OAuth2 confidential-client flow (the only outbound HTTP this hub makes).
//!
//! The auth service exchanges the browser's authorization code (with its PKCE
//! verifier) for Google's tokens, verifies the returned `id_token` against
//! Google's JWKS, checks the `nonce`, and extracts the stable `sub` + verified
//! email. Google's token is then **discarded** — it is never forwarded inward; the
//! hub mints its own first-party token instead.

use std::{
	collections::HashMap,
	sync::Mutex,
	time::{Duration, Instant},
};

use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, jwk::JwkSet};
use serde::Deserialize;

use crate::{AuthError, config::GoogleConfig};

const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
const CERTS_ENDPOINT: &str = "https://www.googleapis.com/oauth2/v3/certs";
const ISSUERS: [&str; 2] = ["https://accounts.google.com", "accounts.google.com"];

/// Floor on how often the certs endpoint may be re-fetched, so a flood of
/// unknown-`kid` id_tokens cannot amplify into a DoS against Google (mirroring the
/// hub verifier's [`MIN_REFRESH_INTERVAL`](crate::verifier)). A refresh also waits
/// out the `Cache-Control: max-age` the last response advertised.
const MIN_CERTS_REFRESH: Duration = Duration::from_secs(30);

/// The verified identity extracted from a Google `id_token`.
#[derive(Debug, Clone)]
pub struct GoogleIdentity {
	pub subject: String,
	pub email: String,
	pub email_verified: bool,
}

/// Google's signing keys (by `kid`) with the freshness window from the last fetch.
#[derive(Default)]
struct CertCache {
	keys: HashMap<String, DecodingKey>,
	/// When the cached keys may next be refreshed: the later of the response's
	/// `Cache-Control: max-age` expiry and [`MIN_CERTS_REFRESH`]. `None` until the
	/// first fetch.
	refresh_after: Option<Instant>,
}

/// A configured Google OAuth2 client.
pub struct GoogleOauth {
	client_id: String,
	client_secret: String,
	http: reqwest::Client,
	certs_endpoint: String,
	certs: Mutex<CertCache>,
}
impl GoogleOauth {
	pub fn new(config: &GoogleConfig) -> Self {
		Self {
			client_id: config.client_id.clone(),
			client_secret: config.client_secret.clone(),
			http: reqwest::Client::new(),
			certs_endpoint: CERTS_ENDPOINT.to_string(),
			certs: Mutex::new(CertCache::default()),
		}
	}

	/// Exchange an authorization code for Google's tokens and return the verified
	/// identity. `nonce` must equal the one the BFF placed in the authorize request.
	pub async fn exchange_code(&self, auth_code: &str, code_verifier: &str, redirect_uri: &str, nonce: &str) -> Result<GoogleIdentity, AuthError> {
		let response = self
			.http
			.post(TOKEN_ENDPOINT)
			.form(&[
				("code", auth_code),
				("client_id", &self.client_id),
				("client_secret", &self.client_secret),
				("redirect_uri", redirect_uri),
				("grant_type", "authorization_code"),
				("code_verifier", code_verifier),
			])
			.send()
			.await
			.map_err(|e| AuthError::Provider(format!("google token request failed: {e}")))?;

		if !response.status().is_success() {
			return Err(AuthError::Provider(format!("google token endpoint returned {}", response.status())));
		}

		let token: GoogleTokenResponse = response.json().await.map_err(|e| AuthError::Provider(format!("malformed google token response: {e}")))?;
		let id_token = token.id_token.ok_or_else(|| AuthError::Provider("google response had no id_token".into()))?;

		self.verify_id_token(&id_token, nonce).await
	}

	async fn verify_id_token(&self, id_token: &str, nonce: &str) -> Result<GoogleIdentity, AuthError> {
		let header = decode_header(id_token).map_err(|_| AuthError::Provider("malformed google id_token header".into()))?;
		if header.alg != Algorithm::RS256 {
			return Err(AuthError::Provider("unexpected google id_token algorithm".into()));
		}
		let kid = header.kid.ok_or_else(|| AuthError::Provider("google id_token missing kid".into()))?;

		let key = match self.cached_key(&kid) {
			Some(key) => key,
			None => {
				self.refresh_certs().await?;
				self.cached_key(&kid).ok_or_else(|| AuthError::Provider("no matching google signing key".into()))?
			}
		};

		let mut validation = Validation::new(Algorithm::RS256);
		validation.set_audience(&[&self.client_id]);
		validation.set_issuer(&ISSUERS);
		validation.set_required_spec_claims(&["exp", "aud", "iss"]);

		let data = decode::<GoogleIdClaims>(id_token, &key, &validation).map_err(|e| AuthError::Provider(format!("google id_token rejected: {e}")))?;
		let claims = data.claims;

		if claims.nonce.as_deref() != Some(nonce) {
			return Err(AuthError::Provider("google id_token nonce mismatch".into()));
		}
		let email = claims.email.ok_or_else(|| AuthError::Provider("google id_token had no email".into()))?;

		Ok(GoogleIdentity {
			subject: claims.sub,
			email,
			email_verified: claims.email_verified.unwrap_or(false),
		})
	}

	fn cached_key(&self, kid: &str) -> Option<DecodingKey> {
		self.certs.lock().expect("certs cache poisoned").keys.get(kid).cloned()
	}

	/// Re-fetch Google's certs, honoring the `Cache-Control: max-age` window of the
	/// previous response (and never more often than [`MIN_CERTS_REFRESH`]) so a flood
	/// of unknown-`kid` id_tokens cannot amplify into a DoS against Google. The cache
	/// is held under a `Mutex` (not across the await): whoever loses the race re-reads
	/// the now-fresh `refresh_after` and skips the redundant call.
	async fn refresh_certs(&self) -> Result<(), AuthError> {
		if let Some(at) = self.certs.lock().expect("certs cache poisoned").refresh_after
			&& Instant::now() < at
		{
			return Ok(());
		}

		let response = self
			.http
			.get(&self.certs_endpoint)
			.send()
			.await
			.map_err(|e| AuthError::Provider(format!("google certs request failed: {e}")))?;
		let max_age = max_age_of(response.headers());
		let certs: JwkSet = response.json().await.map_err(|e| AuthError::Provider(format!("malformed google certs: {e}")))?;

		let mut keys = HashMap::new();
		for jwk in &certs.keys {
			let Some(kid) = jwk.common.key_id.clone() else { continue };
			let key = DecodingKey::from_jwk(jwk).map_err(|e| AuthError::Provider(format!("bad google jwk: {e}")))?;
			keys.insert(kid, key);
		}

		let mut cache = self.certs.lock().expect("certs cache poisoned");
		cache.keys = keys;
		cache.refresh_after = Some(Instant::now() + max_age.max(MIN_CERTS_REFRESH));
		Ok(())
	}
}

/// Parse the `max-age` directive from a `Cache-Control` response header, returning
/// zero when absent or unparseable (so freshness then falls back to [`MIN_CERTS_REFRESH`]).
fn max_age_of(headers: &reqwest::header::HeaderMap) -> Duration {
	headers
		.get(reqwest::header::CACHE_CONTROL)
		.and_then(|v| v.to_str().ok())
		.and_then(|v| v.split(',').filter_map(|d| d.trim().strip_prefix("max-age=")).find_map(|s| s.parse::<u64>().ok()))
		.map(Duration::from_secs)
		.unwrap_or_default()
}

#[derive(Deserialize)]
struct GoogleTokenResponse {
	id_token: Option<String>,
}

#[derive(Deserialize)]
struct GoogleIdClaims {
	sub: String,
	#[serde(default)]
	email: Option<String>,
	#[serde(default)]
	email_verified: Option<bool>,
	#[serde(default)]
	nonce: Option<String>,
}

#[cfg(test)]
mod tests {
	use std::{
		io::{Read, Write},
		net::TcpListener,
		sync::atomic::{AtomicUsize, Ordering},
	};

	use super::*;

	#[test]
	fn parses_max_age_from_cache_control() {
		let mut headers = reqwest::header::HeaderMap::new();
		headers.insert(reqwest::header::CACHE_CONTROL, "public, max-age=3600, must-revalidate".parse().unwrap());
		assert_eq!(max_age_of(&headers), Duration::from_secs(3600));

		let empty = reqwest::header::HeaderMap::new();
		assert_eq!(max_age_of(&empty), Duration::ZERO);
	}

	/// A second refresh inside the cache window must not hit the certs endpoint again,
	/// so each login no longer round-trips Google. Serves one canned JWKS from a local
	/// socket and counts inbound connections.
	#[tokio::test]
	async fn refresh_is_throttled_within_cache_window() {
		static HITS: AtomicUsize = AtomicUsize::new(0);
		let listener = TcpListener::bind("127.0.0.1:0").unwrap();
		let addr = listener.local_addr().unwrap();

		std::thread::spawn(move || {
			for stream in listener.incoming() {
				let mut stream = stream.unwrap();
				let mut buf = [0u8; 1024];
				let _ = stream.read(&mut buf);
				HITS.fetch_add(1, Ordering::SeqCst);
				let body = r#"{"keys":[]}"#;
				let resp = format!(
					"HTTP/1.1 200 OK\r\nCache-Control: max-age=3600\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
					body.len(),
					body
				);
				let _ = stream.write_all(resp.as_bytes());
			}
		});

		let google = GoogleOauth {
			client_id: "test".into(),
			client_secret: "test".into(),
			http: reqwest::Client::new(),
			certs_endpoint: format!("http://{addr}/certs"),
			certs: Mutex::new(CertCache::default()),
		};

		google.refresh_certs().await.unwrap();
		google.refresh_certs().await.unwrap();

		assert_eq!(HITS.load(Ordering::SeqCst), 1, "second refresh within the window must reuse the cache");
	}
}
