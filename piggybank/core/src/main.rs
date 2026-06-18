//! Composition root for the whole `piggybank` system.
//!
//! Loads config, opens the driven infrastructure (Postgres control plane,
//! TigerBeetle ledger), then runs two in-process tasks that talk over the
//! [`Authorizer`] channel:
//!   - the **auth service** ([`evbanking_auth::AuthService`]) — issuance gRPC routes
//!     + the authorize channel, on `auth_grpc_addr`;
//!   - the **core** gRPC services (health/users/balance/allocations) on `grpc_addr`,
//!     authorizing each request via the `Authorizer` core got from auth.

use std::sync::Arc;

use anyhow::Context;
use evbanking_auth::AuthService;
use piggybank_core::{
	AppState,
	config::AppConfig,
	infrastructure::{db, tigerbeetle::TigerBeetle},
	services,
};

// Sentry must be initialised before the async runtime starts — no #[tokio::main].
fn main() -> anyhow::Result<()> {
	dotenvy::dotenv().ok();

	let config = AppConfig::from_env().context("failed to load configuration")?;

	// Guard must stay alive for the duration of main — dropping it flushes events.
	let _sentry_guard = config.sentry_dsn.as_deref().map(|dsn| {
		sentry::init((
			dsn,
			sentry::ClientOptions {
				release: sentry::release_name!(),
				environment: Some(config.app_env.clone().into()),
				traces_sample_rate: if config.app_env == "production" { 0.1 } else { 1.0 },
				..Default::default()
			},
		))
	});

	init_tracing();

	tokio::runtime::Builder::new_multi_thread()
		.enable_all()
		.build()
		.context("failed to build tokio runtime")?
		.block_on(run(config))
}

async fn run(config: AppConfig) -> anyhow::Result<()> {
	// ── driven infrastructure ─────────────────────────────────────────────────
	let pool = db::connect(&config.database_url).await.context("failed to connect to the database")?;
	let tigerbeetle = Arc::new(TigerBeetle::connect(config.tigerbeetle_cluster_id, &config.tigerbeetle_address).context("failed to connect to TigerBeetle")?);

	// ── auth service (its own task) ───────────────────────────────────────────
	// Auth owns the keys/JWKS and hands core an `Authorizer` channel handle.
	let (auth_service, authorizer) = AuthService::new();
	let state = AppState::new(pool, tigerbeetle, authorizer);

	let auth = tokio::spawn(auth_service.run(config.auth_grpc_addr));

	// ── core gRPC (this task) ─────────────────────────────────────────────────
	tracing::info!(core = %config.grpc_addr, auth = %config.auth_grpc_addr, "piggybank listening");

	// Run both; whichever ends first (error or shutdown) tears the process down.
	tokio::select! {
		result = services::serve(config.grpc_addr, state) => result.context("core gRPC server error")?,
		result = auth => result.context("auth task failed")?.context("auth service error")?,
	}
	Ok(())
}

fn init_tracing() {
	use tracing_subscriber::{EnvFilter, fmt, prelude::*};

	let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,piggybank_core=debug,evbanking_auth=debug"));
	tracing_subscriber::registry().with(filter).with(fmt::layer()).with(sentry::integrations::tracing::layer()).init();
}
