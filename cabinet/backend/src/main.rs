//! The cabinet BFF — a standalone, stateless HTTP orchestration service.
//!
//! It is the cabinet's single auth/egress boundary: it runs the OAuth confidential-client
//! flow, holds the user's session (and the concierge token pair) server-side, and proxies
//! the browser's same-origin `/api/*` JSON requests to two gRPC planes — **concierge**
//! (identity: OAuth, sessions, profile) and **piggybank** (money: wallet, funds, health).
//! The Next.js frontend reaches it via a same-origin rewrite, so the browser only ever
//! holds an opaque session cookie + a readable CSRF cookie.

use std::sync::Arc;

use anyhow::Context;
use ev::error_monitoring::{self, Config as SentryConfig};

mod config;
mod cookies;
mod dto;
mod error;
mod oauth;
mod routes;
mod session;
mod state;
mod util;

use config::Config;
use cookies::CookieNames;
use oauth::OAuthTxStore;
use session::SessionStore;
use state::{AppState, Grpc};

// Sentry must be initialised before the async runtime starts — no `#[tokio::main]`.
fn main() -> anyhow::Result<()> {
	dotenvy::dotenv().ok();

	let config = Config::from_env().context("failed to load configuration")?;

	// Guard must stay alive for the duration of main — dropping it flushes events. A
	// `None` DSN makes `init` return `None`, so this binding is simply inert.
	let _sentry_guard = error_monitoring::init(&SentryConfig {
		dsn: config.sentry_dsn.clone(),
		environment: config.app_env.clone(),
		traces_sample_rate: SentryConfig::traces_sample_rate_for(&config.app_env),
	});

	init_tracing();

	tokio::runtime::Builder::new_multi_thread()
		.enable_all()
		.build()
		.context("failed to build tokio runtime")?
		.block_on(run(config))
}

async fn run(config: Config) -> anyhow::Result<()> {
	// Product-analytics capture (native PostHog). A `None` key makes capture a silent
	// no-op, so this is safe to construct unconfigured.
	let _analytics = ev::analytics::Analytics::new(config.posthog_key.clone(), config.posthog_host.clone());

	let grpc = Grpc::connect_lazy(
		&config.piggybank_grpc_addr,
		&config.banking_auth_grpc_addr,
		&config.concierge_grpc_addr,
		config.banking_issuance_token.as_ref().map(|t| t.0.clone()),
	)
	.context("invalid gRPC address (PIGGYBANK_GRPC_ADDR / BANKING_AUTH_GRPC_ADDR / CONCIERGE_GRPC_ADDR)")?;

	let bind_addr = config.bind_addr;
	tracing::info!(
		bind = %bind_addr,
		piggybank = %config.piggybank_grpc_addr,
		concierge = %config.concierge_grpc_addr,
		"cabinet BFF listening"
	);

	let state = AppState {
		cookies: Arc::new(CookieNames::new(config.cookie_secure)),
		sessions: Arc::new(SessionStore::from_env().await.context("failed to initialize the session store")?),
		oauth: Arc::new(OAuthTxStore::new()),
		grpc,
		config: Arc::new(config),
	};

	let listener = tokio::net::TcpListener::bind(bind_addr).await.context("failed to bind HTTP listener")?;
	axum::serve(listener, routes::router(state)).await.context("cabinet BFF HTTP server error")
}

fn init_tracing() {
	use tracing_subscriber::{EnvFilter, fmt, prelude::*};

	let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,cabinet_backend=debug"));
	tracing_subscriber::registry().with(filter).with(fmt::layer()).with(error_monitoring::tracing_layer()).init();
}
