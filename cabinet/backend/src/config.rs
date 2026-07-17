use std::net::SocketAddr;

ev::settings! {
	/// Runtime configuration for the cabinet BFF, read from the environment only
	/// (`dotenvy` loads `.env` first in dev; prod values ride the container env —
	/// the image's baked topology contract plus gitops' k8s Secret `envFrom`).
	/// Every missing/invalid variable is reported in one aggregate error before
	/// anything binds, and an empty string counts as unset.
	pub struct AppConfig {
		/// HTTP listener — the address the Next.js frontend's `/api/*` rewrite points at.
		/// Loopback by default: the BFF's request-auth is a bearer cookie, so it must sit
		/// behind the same-origin reverse proxy unless an operator opts into a wider bind
		/// (and pairs it with network segmentation — see `docs/ARCHITECTURE.md`).
		/// Named `CABINET_BACKEND_BIND` (not a bare `BIND`) so the deployment env stays
		/// unambiguous next to the other services' binds.
		#[env("CABINET_BACKEND_BIND")]
		bind: SocketAddr = "127.0.0.1:4000",
		/// The piggybank money plane (wallet/funds/health), e.g. `http://127.0.0.1:50051`.
		piggybank_grpc_addr: String,
		/// The banking auth service (token issuance), e.g. `http://127.0.0.1:50052`. The BFF
		/// calls `IssueUserToken`/`Refresh` here to obtain the money-plane token pair.
		banking_auth_grpc_addr: String,
		/// The shared bearer the BFF presents on `IssueUserToken` (the concierge→banking seam).
		/// Must match `BANKING_ISSUANCE_TOKEN` on the banking auth side.
		#[secret]
		banking_issuance_token: IssuanceToken,
		/// The concierge identity plane (directory/profile), e.g. `http://127.0.0.1:50061`.
		/// Also serves the JWKS the access-JWT verifier caches (auth is shell-owned; the
		/// BFF only VERIFIES the shared `ev_access` cookie, it never runs OAuth).
		concierge_grpc_addr: String,
		/// Issuer + audience the verifier expects on the shared access JWT. Must match
		/// what the concierge plane stamps (its `AUTH_ISSUER` / `AUTH_CLIENT_AUDIENCE`).
		auth_issuer: String,
		auth_client_audience: String,
		/// Explicit override for cookie hardening; unset ⇒ infer from `app_env`
		/// (`__Host-`/Secure needs HTTPS, which dev lacks). See [`Self::cookie_secure`].
		auth_cookie_secure: Option<bool>,
		/// Path to the microfrontend registry served at `/api/mfe-registry`.
		mfe_registry_path: String,
		/// Cross-origin MFE bundle origins allowed beyond same-origin (relative) URLs.
		/// Mirrors the frontend CSP allow-list (`MFE_ALLOWED_ORIGINS`); the registry is
		/// validated against it before being served, so a poisoned entry never reaches the
		/// browser. Same-origin (relative) `scriptUrl`s need no entry.
		mfe_allowed_origins: Vec<String> = "",
		app_env: String,
		sentry_dsn: Option<String>,
		posthog_key: Option<String>,
		posthog_host: Option<String>,
	}
}

impl AppConfig {
	/// Whether cookies are `__Host-`-prefixed + `Secure` (production over HTTPS).
	pub fn cookie_secure(&self) -> bool {
		self.auth_cookie_secure.unwrap_or(self.app_env == "production")
	}
}

/// The issuance bearer as a newtype with a hand-written `Debug`, so the secret is never
/// printed by a derived debug (mirrors `evbanking_auth`'s `IssuanceToken`). The settings
/// `#[secret]` layer additionally redacts it in the generated `Debug`/error output.
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
ev::settings_via_from_str!(IssuanceToken);

#[cfg(test)]
mod tests {
	use std::collections::HashMap;

	use super::*;

	fn minimal_env() -> HashMap<String, String> {
		[
			("PIGGYBANK_GRPC_ADDR", "http://127.0.0.1:50051"),
			("BANKING_AUTH_GRPC_ADDR", "http://127.0.0.1:50052"),
			("BANKING_ISSUANCE_TOKEN", "test-issuance"),
			("CONCIERGE_GRPC_ADDR", "http://127.0.0.1:50061"),
			("AUTH_ISSUER", "https://auth.concierge.ev"),
			("AUTH_CLIENT_AUDIENCE", "concierge"),
			("MFE_REGISTRY_PATH", "/mfe-registry.json"),
			("APP_ENV", "development"),
		]
		.into_iter()
		.map(|(k, v)| (k.to_string(), v.to_string()))
		.collect()
	}

	#[test]
	fn default_bind_is_loopback() {
		let config = AppConfig::from_source(|var| minimal_env().get(var).cloned()).expect("minimal env loads");
		assert!(config.bind.ip().is_loopback(), "the BFF must default to loopback, not all interfaces");
	}

	/// The env surface IS the deploy contract (the flake image env + gitops
	/// manifests use exactly these names) — a rename here must be deliberate.
	#[test]
	fn env_surface_matches_the_deploy_contract() {
		assert_eq!(
			AppConfig::var_names(),
			vec![
				"CABINET_BACKEND_BIND",
				"PIGGYBANK_GRPC_ADDR",
				"BANKING_AUTH_GRPC_ADDR",
				"BANKING_ISSUANCE_TOKEN",
				"CONCIERGE_GRPC_ADDR",
				"AUTH_ISSUER",
				"AUTH_CLIENT_AUDIENCE",
				"AUTH_COOKIE_SECURE",
				"MFE_REGISTRY_PATH",
				"MFE_ALLOWED_ORIGINS",
				"APP_ENV",
				"SENTRY_DSN",
				"POSTHOG_KEY",
				"POSTHOG_HOST",
			]
		);
	}
}
