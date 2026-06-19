//! Google OAuth2 confidential-client flow (the only outbound HTTP this hub makes).
//!
//! The auth service exchanges the browser's authorization code (with its PKCE
//! verifier) for Google's tokens, verifies the returned `id_token` against
//! Google's JWKS, checks the `nonce`, and extracts the stable `sub` + verified
//! email. Google's token is then **discarded** — it is never forwarded inward; the
//! hub mints its own first-party token instead.

use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, jwk::JwkSet};
use serde::Deserialize;

use crate::{AuthError, config::GoogleConfig};

const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
const CERTS_ENDPOINT: &str = "https://www.googleapis.com/oauth2/v3/certs";
const ISSUERS: [&str; 2] = ["https://accounts.google.com", "accounts.google.com"];

/// The verified identity extracted from a Google `id_token`.
#[derive(Debug, Clone)]
pub struct GoogleIdentity {
	pub subject: String,
	pub email: String,
	pub email_verified: bool,
}

/// A configured Google OAuth2 client.
pub struct GoogleOauth {
	client_id: String,
	client_secret: String,
	http: reqwest::Client,
}
impl GoogleOauth {
	pub fn new(config: &GoogleConfig) -> Self {
		Self {
			client_id: config.client_id.clone(),
			client_secret: config.client_secret.clone(),
			http: reqwest::Client::new(),
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

		let certs: JwkSet = self
			.http
			.get(CERTS_ENDPOINT)
			.send()
			.await
			.map_err(|e| AuthError::Provider(format!("google certs request failed: {e}")))?
			.json()
			.await
			.map_err(|e| AuthError::Provider(format!("malformed google certs: {e}")))?;

		let jwk = certs
			.keys
			.iter()
			.find(|k| k.common.key_id.as_deref() == Some(&kid))
			.ok_or_else(|| AuthError::Provider("no matching google signing key".into()))?;
		let key = DecodingKey::from_jwk(jwk).map_err(|e| AuthError::Provider(format!("bad google jwk: {e}")))?;

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
