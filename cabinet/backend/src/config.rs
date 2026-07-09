use std::net::SocketAddr;

use anyhow::Context;

/// Default HTTP bind: loopback, so the token-holding BFF is reachable only through the
/// frontend's same-origin `/api/*` reverse proxy unless an operator opts into a wider
/// bind (and pairs it with network segmentation — see `docs/ARCHITECTURE.md`).
const DEFAULT_BIND: &str = "127.0.0.1:4000";

/// Runtime configuration for the cabinet BFF, sourced from environment variables
/// (and `cabinet/backend/.env` in development via `dotenvy`).
#[derive(Clone)]
pub struct Config {
	/// HTTP listener — the address the Next.js frontend's `/api/*` rewrite points at.
	pub bind_addr: SocketAddr,
	/// The piggybank money plane (wallet/funds/health), e.g. `http://127.0.0.1:50051`.
	pub piggybank_grpc_addr: String,
	/// The banking auth service (token issuance), e.g. `http://127.0.0.1:50052`. The BFF
	/// calls `IssueUserToken`/`Refresh` here to obtain the money-plane token pair.
	pub banking_auth_grpc_addr: String,
	/// The shared bearer the BFF presents on `IssueUserToken` (the concierge→banking seam).
	/// `None` ⇒ no money-plane token is minted (money routes surface `NotConfigured`). Must
	/// match `BANKING_ISSUANCE_TOKEN` on the banking auth side.
	pub banking_issuance_token: Option<IssuanceToken>,
	/// The concierge identity plane (directory/profile), e.g. `http://127.0.0.1:50061`.
	/// Also serves the JWKS the access-JWT verifier caches (auth is shell-owned; the
	/// BFF only VERIFIES the shared `ev_access` cookie, it never runs OAuth).
	pub concierge_grpc_addr: String,
	/// Issuer + audience the verifier expects on the shared access JWT. Must match
	/// what the concierge plane stamps (its `AUTH_ISSUER` / `AUTH_CLIENT_AUDIENCE`).
	pub auth_issuer: String,
	pub auth_client_audience: String,
	/// Whether cookies are `__Host-`-prefixed + `Secure` (production over HTTPS).
	pub cookie_secure: bool,
	/// Path to the microfrontend registry served at `/api/mfe-registry`.
	pub mfe_registry_path: String,
	/// Cross-origin MFE bundle origins allowed beyond same-origin (relative) URLs.
	/// Mirrors the frontend CSP allow-list (`MFE_ALLOWED_ORIGINS`); the registry is
	/// validated against it before being served, so a poisoned entry never reaches the
	/// browser. Same-origin (relative) `scriptUrl`s need no entry.
	pub mfe_allowed_origins: Vec<String>,
	pub app_env: String,
	pub sentry_dsn: Option<String>,
	pub posthog_key: Option<String>,
	pub posthog_host: Option<String>,
}
impl Config {
	pub fn from_env() -> anyhow::Result<Self> {
		let app_env = std::env::var("APP_ENV").unwrap_or_else(|_| "development".to_string());
		// Mirrors the frontend's cookie logic: explicit AUTH_COOKIE_SECURE wins, else
		// infer from the environment (`__Host-`/Secure needs HTTPS, which dev lacks).
		let cookie_secure = match opt("AUTH_COOKIE_SECURE") {
			Some(v) => v == "true",
			None => app_env == "production",
		};
		// Loopback by default: the BFF's request-auth is a bearer cookie, so it must sit
		// behind the same-origin reverse proxy. Opt into a wider bind (e.g. 0.0.0.0:4000)
		// only with an upstream firewall in place.
		let bind_addr = std::env::var("CABINET_BACKEND_BIND")
			.unwrap_or_else(|_| DEFAULT_BIND.to_string())
			.parse()
			.with_context(|| format!("CABINET_BACKEND_BIND must be a valid socket address, e.g. {DEFAULT_BIND}"))?;
		Ok(Self {
			bind_addr,
			piggybank_grpc_addr: std::env::var("PIGGYBANK_GRPC_ADDR").unwrap_or_else(|_| "http://127.0.0.1:50051".to_string()),
			banking_auth_grpc_addr: std::env::var("BANKING_AUTH_GRPC_ADDR").unwrap_or_else(|_| "http://127.0.0.1:50052".to_string()),
			banking_issuance_token: opt("BANKING_ISSUANCE_TOKEN").map(IssuanceToken),
			concierge_grpc_addr: std::env::var("CONCIERGE_GRPC_ADDR").unwrap_or_else(|_| "http://127.0.0.1:50061".to_string()),
			// Defaults mirror the concierge plane's own (`assert_plane`-checked) values.
			auth_issuer: std::env::var("AUTH_ISSUER").unwrap_or_else(|_| "https://auth.concierge.ev".to_string()),
			auth_client_audience: std::env::var("AUTH_CLIENT_AUDIENCE").unwrap_or_else(|_| "concierge".to_string()),
			cookie_secure,
			mfe_registry_path: std::env::var("MFE_REGISTRY_PATH").unwrap_or_else(|_| "cabinet/frontend/mfe-registry.json".to_string()),
			mfe_allowed_origins: opt("MFE_ALLOWED_ORIGINS")
				.map(|v| v.split([' ', ',', '\t', '\n']).filter(|s| !s.is_empty()).map(str::to_string).collect())
				.unwrap_or_default(),
			app_env,
			sentry_dsn: opt("SENTRY_DSN"),
			posthog_key: opt("POSTHOG_KEY"),
			posthog_host: opt("POSTHOG_HOST"),
		})
	}
}

/// The issuance bearer as a newtype with a hand-written `Debug`, so the secret is never
/// printed by a derived debug (mirrors `evbanking_auth`'s `IssuanceToken`).
#[derive(Clone)]
pub struct IssuanceToken(pub String);
impl std::fmt::Debug for IssuanceToken {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str("IssuanceToken(<redacted>)")
	}
}

fn opt(key: &str) -> Option<String> {
	std::env::var(key).ok().filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn default_bind_is_loopback() {
		let addr: SocketAddr = DEFAULT_BIND.parse().expect("default bind parses");
		assert!(addr.ip().is_loopback(), "the BFF must default to loopback, not all interfaces");
	}
}
