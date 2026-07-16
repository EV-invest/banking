//! The cabinet BFF — a standalone, stateless HTTP orchestration service.
//!
//! It is the cabinet's egress boundary: it proxies the browser's same-origin `/api/*`
//! JSON requests to two gRPC planes — **concierge** (identity: profile/directory) and
//! **piggybank** (money: wallet, funds, health). Auth is SHELL-owned: the concierge
//! auth web surface (reached via the conductor's `/api/auth/*` on the shared origin)
//! signs users in and sets the zone-shared `ev_access` JWT cookie; the BFF verifies
//! that cookie locally against the concierge JWKS and, for money routes, mints the
//! SEPARATE banking (`aud=banking-core`) pair for the verified subject. The cabinet
//! runs no OAuth and holds no session.

use std::sync::Arc;

use color_eyre::eyre::Context;
use ev::error_monitoring::{self, Config as SentryConfig};
use evconcierge_auth::{Verifier, VerifierConfig};

mod config;
mod cookies;
mod dto;
mod error;
mod routes;
mod session;
mod state;
mod util;

use clap::Parser;
use config::AppConfig;
use cookies::CookieNames;
use session::BankingTokens;
use state::{AppState, Grpc};

#[derive(Parser)]
struct Cli {
	#[clap(flatten)]
	settings_flags: config::SettingsFlags,
}

// Sentry must be initialised before the async runtime starts — no `#[tokio::main]`.
fn main() -> color_eyre::Result<()> {
	color_eyre::install()?;
	dotenvy::dotenv().ok();

	let cli = Cli::parse();
	// One snapshot at boot; hot reload is unused. A missing `{ env = "VAR" }` ref
	// in the prod config fails HERE, before anything binds.
	let settings = v_utils::utils::exit_on_error(config::LiveSettings::new(cli.settings_flags, std::time::Duration::from_secs(60)));
	let config = v_utils::utils::exit_on_error(settings.config());

	// Guard must stay alive for the duration of main — dropping it flushes events. A
	// `None` DSN makes `init` return `None`, so this binding is simply inert.
	let _sentry_guard = error_monitoring::init(&SentryConfig {
		dsn: config.sentry_dsn.clone(),
		environment: config.app_env.clone(),
		traces_sample_rate: SentryConfig::traces_sample_rate_for(&config.app_env),
	});

	// Held for the process lifetime — dropping flushes OTel logs/traces.
	let _otel_guard = init_tracing(&config.app_env);

	tokio::runtime::Builder::new_multi_thread()
		.enable_all()
		.build()
		.context("failed to build tokio runtime")?
		.block_on(run(config))
}

async fn run(config: AppConfig) -> color_eyre::Result<()> {
	// Product-analytics capture (native PostHog). A `None` key makes capture a silent
	// no-op, so this is safe to construct unconfigured.
	let _analytics = ev::analytics::Analytics::new(config.posthog_key.clone(), config.posthog_host.clone());

	let grpc = Grpc::connect_lazy(
		&config.piggybank_grpc_addr,
		&config.banking_auth_grpc_addr,
		&config.concierge_grpc_addr,
		Some(config.banking_issuance_token.0.clone()),
	)
	.context("invalid gRPC address (PIGGYBANK_GRPC_ADDR / BANKING_AUTH_GRPC_ADDR / CONCIERGE_GRPC_ADDR)")?;

	let bind_addr = config.bind;
	tracing::info!(
		bind = %bind_addr,
		piggybank = %config.piggybank_grpc_addr,
		concierge = %config.concierge_grpc_addr,
		"cabinet BFF listening"
	);

	// Local verification of the shell-set access JWT: the JWKS is cached from the
	// concierge plane's public `Jwks` RPC (lazy — the first verify warms it), so no
	// per-request round trip. Fails closed until the plane publishes keys.
	let verifier = Verifier::try_new(VerifierConfig {
		issuer: config.auth_issuer.clone(),
		audiences: vec![config.auth_client_audience.clone()],
		allowed_types: vec![evconcierge_auth::TokenType::Access],
		jwks_grpc_endpoint: std::env::var("AUTH_JWKS_GRPC_ENDPOINT").unwrap_or_else(|_| config.concierge_grpc_addr.clone()),
	})
	.context("failed to build the access-token verifier")?;

	let state = AppState {
		cookies: Arc::new(CookieNames::new(config.cookie_secure())),
		banking: Arc::new(BankingTokens::new()),
		verifier,
		grpc,
		config: Arc::new(config),
	};

	let listener = tokio::net::TcpListener::bind(bind_addr).await.context("failed to bind HTTP listener")?;
	axum::serve(listener, routes::router(state)).await.context("cabinet BFF HTTP server error")
}

// Returns the OTel guard (flushes/shuts down on drop); bind it in `main`. `None`
// when OTEL_EXPORTER_OTLP_ENDPOINT is unset — the layers are then inert.
fn init_tracing(environment: &str) -> Option<ev::otel::Telemetry> {
	use tracing_subscriber::{EnvFilter, fmt, prelude::*};

	let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,cabinet_backend=debug"));
	let (otel_guard, otel_layers) = ev::otel::telemetry(&ev::otel::Config {
		environment: environment.to_string(),
		traces_sample_rate: ev::otel::Config::traces_sample_rate_for(environment),
	})
	.unzip();
	tracing_subscriber::registry()
		.with(filter)
		.with(fmt::layer().json())
		.with(error_monitoring::tracing_layer())
		.with(otel_layers)
		.init();
	otel_guard
}
