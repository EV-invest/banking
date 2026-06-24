use std::net::SocketAddr;

use anyhow::Context;

/// Runtime configuration for the cabinet BFF, sourced from environment variables
/// (and `clients/cabinet/backend/.env` in development via `dotenvy`).
#[derive(Clone)]
pub struct Config {
	/// HTTP listener — the address the Next.js frontend's `/api/*` rewrite points at.
	pub bind_addr: SocketAddr,
	/// The piggybank money plane (wallet/funds/health), e.g. `http://127.0.0.1:50051`.
	pub piggybank_grpc_addr: String,
	/// The concierge identity plane (OAuth/sessions/profile), e.g. `http://127.0.0.1:50061`.
	pub concierge_grpc_addr: String,
	/// Google OAuth2 public client id. `None` ⇒ `/api/auth/login` returns 503.
	pub google_client_id: Option<String>,
	/// The OAuth redirect URI registered with Google (the browser-facing callback URL).
	pub auth_redirect_uri: String,
	/// Whether cookies are `__Host-`-prefixed + `Secure` (production over HTTPS).
	pub cookie_secure: bool,
	/// Path to the microfrontend registry served at `/api/mfe-registry`.
	pub mfe_registry_path: String,
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
		let bind_addr = std::env::var("CABINET_BACKEND_BIND")
			.unwrap_or_else(|_| "0.0.0.0:4000".to_string())
			.parse()
			.context("CABINET_BACKEND_BIND must be a valid socket address, e.g. 0.0.0.0:4000")?;
		Ok(Self {
			bind_addr,
			piggybank_grpc_addr: std::env::var("PIGGYBANK_GRPC_ADDR").unwrap_or_else(|_| "http://127.0.0.1:50051".to_string()),
			concierge_grpc_addr: std::env::var("CONCIERGE_GRPC_ADDR").unwrap_or_else(|_| "http://127.0.0.1:50061".to_string()),
			google_client_id: opt("GOOGLE_CLIENT_ID"),
			auth_redirect_uri: std::env::var("AUTH_REDIRECT_URI").unwrap_or_else(|_| "http://localhost:3000/api/auth/callback".to_string()),
			cookie_secure,
			mfe_registry_path: std::env::var("MFE_REGISTRY_PATH").unwrap_or_else(|_| "clients/cabinet/frontend/mfe-registry.json".to_string()),
			app_env,
			sentry_dsn: opt("SENTRY_DSN"),
			posthog_key: opt("POSTHOG_KEY"),
			posthog_host: opt("POSTHOG_HOST"),
		})
	}
}

fn opt(key: &str) -> Option<String> {
	std::env::var(key).ok().filter(|v| !v.is_empty())
}
