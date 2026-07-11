use std::net::SocketAddr;

use smart_default::SmartDefault;
use v_utils::macros as v_macros;

/// Default HTTP bind: loopback, so the token-holding BFF is reachable only through the
/// frontend's same-origin `/api/*` reverse proxy unless an operator opts into a wider
/// bind (and pairs it with network segmentation — see `docs/ARCHITECTURE.md`).
const DEFAULT_BIND: &str = "127.0.0.1:4000";

/// Runtime configuration for the cabinet BFF (LiveSettings). Prod runs `--config`
/// on the baked `deploy/cabinet-backend.nix` result — `{ env = "VAR" }` refs there
/// assert the var's presence at startup. Dev runs config-less from the
/// flake-exported env (`#[settings(use_env = true)]` aliases each field to its
/// SHOUTY name).
#[derive(Clone, Debug, v_macros::LiveSettings, v_macros::MyConfigPrimitives, v_macros::Settings, SmartDefault)]
#[settings(use_env = true)]
pub struct AppConfig {
	/// HTTP listener — the address the Next.js frontend's `/api/*` rewrite points at.
	/// Loopback by default: the BFF's request-auth is a bearer cookie, so it must sit
	/// behind the same-origin reverse proxy.
	#[default(DEFAULT_BIND.parse().unwrap())]
	pub bind: SocketAddr,
	/// The piggybank money plane (wallet/funds/health), e.g. `http://127.0.0.1:50051`.
	pub piggybank_grpc_addr: String,
	/// The banking auth service (token issuance), e.g. `http://127.0.0.1:50052`. The BFF
	/// calls `IssueUserToken`/`Refresh` here to obtain the money-plane token pair.
	pub banking_auth_grpc_addr: String,
	/// The shared bearer the BFF presents on `IssueUserToken` (the concierge→banking seam).
	/// Must match `BANKING_ISSUANCE_TOKEN` on the banking auth side.
	#[private_value]
	pub banking_issuance_token: IssuanceToken,
	/// The concierge identity plane (directory/profile), e.g. `http://127.0.0.1:50061`.
	/// Also serves the JWKS the access-JWT verifier caches (auth is shell-owned; the
	/// BFF only VERIFIES the shared `ev_access` cookie, it never runs OAuth).
	pub concierge_grpc_addr: String,
	/// Issuer + audience the verifier expects on the shared access JWT. Must match
	/// what the concierge plane stamps (its `AUTH_ISSUER` / `AUTH_CLIENT_AUDIENCE`).
	pub auth_issuer: String,
	pub auth_client_audience: String,
	/// Explicit override for cookie hardening; unset ⇒ infer from `app_env`
	/// (`__Host-`/Secure needs HTTPS, which dev lacks). See [`Self::cookie_secure`].
	pub auth_cookie_secure: Option<bool>,
	/// Path to the microfrontend registry served at `/api/mfe-registry`.
	pub mfe_registry_path: String,
	/// Cross-origin MFE bundle origins allowed beyond same-origin (relative) URLs.
	/// Mirrors the frontend CSP allow-list (`MFE_ALLOWED_ORIGINS`); the registry is
	/// validated against it before being served, so a poisoned entry never reaches the
	/// browser. Same-origin (relative) `scriptUrl`s need no entry.
	#[serde(default)]
	pub mfe_allowed_origins: Vec<String>,
	pub app_env: String,
	pub sentry_dsn: Option<String>,
	pub posthog_key: Option<String>,
	pub posthog_host: Option<String>,
}
impl AppConfig {
	/// Whether cookies are `__Host-`-prefixed + `Secure` (production over HTTPS).
	pub fn cookie_secure(&self) -> bool {
		self.auth_cookie_secure.unwrap_or(self.app_env == "production")
	}
}

/// The issuance bearer as a newtype with a hand-written `Debug`, so the secret is never
/// printed by a derived debug (mirrors `evbanking_auth`'s `IssuanceToken`).
#[derive(Clone, Default)]
pub struct IssuanceToken(pub String);
impl std::fmt::Debug for IssuanceToken {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str("IssuanceToken(<redacted>)")
	}
}
impl std::str::FromStr for IssuanceToken {
	type Err = std::convert::Infallible;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		Ok(Self(s.to_owned()))
	}
}
// Required by the Settings machinery (config write-defaults/diff); transparent,
// like the `{ env = ... }`-resolved string it round-trips to.
impl serde::Serialize for IssuanceToken {
	fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
		serializer.serialize_str(&self.0)
	}
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
