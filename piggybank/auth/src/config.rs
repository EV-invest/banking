//! Auth configuration — the issuing side ([`AuthConfig`], host-only) and the
//! downstream verifying side ([`VerifierConfig`]).
//!
//! Both read from the environment so the same binary runs unconfigured in local
//! dev and CI: with no signing key, the auth service mints nothing and authorizes
//! nothing (it answers [`AuthError::NotConfigured`](crate::AuthError)), exactly
//! like the rest of the scaffold's no-op-until-configured seams.

use std::env;

use anyhow::Context;

use crate::claims::TokenType;

/// Configuration for the auth **service** (the token-issuing side). Construct with
/// [`AuthConfig::from_env`] in the composition root and hand to
/// [`AuthService::try_new`](crate::service::AuthService::try_new).
#[derive(Clone, Debug)]
pub struct AuthConfig {
	/// Issuer (`iss`) stamped on every minted token and required by verifiers.
	pub issuer: String,
	/// Audience for client access tokens (the hub data plane).
	pub client_audience: String,
	/// Audience for inter-service tokens.
	pub service_audience: String,
	/// Access-token TTL (seconds). Short by design (5–15 min).
	pub access_ttl_secs: u64,
	/// Refresh-token TTL (seconds).
	pub refresh_ttl_secs: u64,
	/// Service-token TTL (seconds).
	pub service_ttl_secs: u64,
	/// Signing/verification key material. `None` ⇒ auth disabled (dev/CI).
	pub signing: Option<SigningConfig>,
	/// Google OAuth2 client credentials. `None` ⇒ the `Exchange` route is disabled.
	pub google: Option<GoogleConfig>,
}
impl AuthConfig {
	pub fn from_env() -> anyhow::Result<Self> {
		let signing = match (env::var("AUTH_SIGNING_KEY_PEM").ok(), env::var("AUTH_SIGNING_KID").ok(), env::var("AUTH_JWKS_JSON").ok()) {
			(Some(pem), Some(kid), Some(jwks)) if !pem.is_empty() && !kid.is_empty() && !jwks.is_empty() => Some(SigningConfig {
				signing_key_pem: pem,
				kid,
				jwks_json: jwks,
			}),
			_ => None,
		};
		let google = match (
			env::var("GOOGLE_CLIENT_ID").ok().filter(|s| !s.is_empty()),
			env::var("GOOGLE_CLIENT_SECRET").ok().filter(|s| !s.is_empty()),
		) {
			(Some(client_id), Some(client_secret)) => Some(GoogleConfig { client_id, client_secret }),
			_ => None,
		};
		Ok(Self {
			issuer: env::var("AUTH_ISSUER").unwrap_or_else(|_| "https://auth.banking.ev".to_string()),
			client_audience: env::var("AUTH_CLIENT_AUDIENCE").unwrap_or_else(|_| "banking-core".to_string()),
			service_audience: env::var("AUTH_SERVICE_AUDIENCE").unwrap_or_else(|_| "banking-services".to_string()),
			access_ttl_secs: parse_secs("AUTH_ACCESS_TTL_SECS", 900)?,
			refresh_ttl_secs: parse_secs("AUTH_REFRESH_TTL_SECS", 2_592_000)?,
			service_ttl_secs: parse_secs("AUTH_SERVICE_TTL_SECS", 300)?,
			signing,
			google,
		})
	}
}

/// The hub's own signing key plus the public JWKS it publishes and verifies against.
#[derive(Clone, Debug)]
pub struct SigningConfig {
	/// Ed25519 private key, PKCS#8 PEM (`AUTH_SIGNING_KEY_PEM`).
	pub signing_key_pem: String,
	/// Key id stamped in the JWT header and matched on verify (`AUTH_SIGNING_KID`).
	pub kid: String,
	/// Public JWKS as a JSON `{"keys":[...]}` (`AUTH_JWKS_JSON`) — served by the
	/// `Jwks` RPC and parsed into the verification key ring. Includes the active
	/// `kid` and any retired-but-still-valid keys (make-before-break rotation).
	pub jwks_json: String,
}

/// Google OAuth2 confidential-client credentials.
#[derive(Clone, Debug)]
pub struct GoogleConfig {
	pub client_id: String,
	pub client_secret: String,
}

/// Configuration a **downstream service** uses to build a [`Verifier`](crate::verifier::Verifier).
#[derive(Clone, Debug)]
pub struct VerifierConfig {
	/// Expected issuer (`iss`).
	pub issuer: String,
	/// Accepted audiences — a SET, not a single value, so the hub's own in-process
	/// verifier can accept the variety of audiences the hub itself issues.
	pub audiences: Vec<String>,
	/// Accepted token types (e.g. `[Access]` for a user-facing service).
	pub allowed_types: Vec<TokenType>,
	/// gRPC address of the hub auth service, dialed to refresh the JWKS cache.
	pub jwks_grpc_endpoint: String,
}

impl VerifierConfig {
	pub fn from_env() -> anyhow::Result<Self> {
		Ok(Self {
			issuer: env::var("AUTH_ISSUER").unwrap_or_else(|_| "https://auth.banking.ev".to_string()),
			audiences: split_csv(&env::var("AUTH_CLIENT_AUDIENCE").unwrap_or_else(|_| "banking-core".to_string())),
			allowed_types: vec![TokenType::Access],
			jwks_grpc_endpoint: env::var("AUTH_JWKS_GRPC_ENDPOINT").context("AUTH_JWKS_GRPC_ENDPOINT must be set for a downstream verifier")?,
		})
	}
}

fn parse_secs(key: &str, default: u64) -> anyhow::Result<u64> {
	match env::var(key) {
		Ok(v) if !v.is_empty() => v.parse().with_context(|| format!("{key} must be a positive integer (seconds)")),
		_ => Ok(default),
	}
}

fn split_csv(raw: &str) -> Vec<String> {
	raw.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect()
}
