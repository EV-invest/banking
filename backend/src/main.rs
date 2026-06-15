//! Composition root.
//!
//! Wiring lives here and only here: load config, open the driven infrastructure
//! (Postgres pool, TigerBeetle client), then mount BOTH driving adapters — the
//! Axum HTTP API and the tonic gRPC API — and serve them concurrently.

use std::sync::Arc;

use anyhow::Context;
use backend::{
	api::{self, state::AppState},
	config::AppConfig,
	grpc,
	infrastructure::{db, tigerbeetle::TigerBeetle},
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
	let state = AppState::new(pool, tigerbeetle);

	// ── HTTP (Axum) ───────────────────────────────────────────────────────────
	let http_router = api::router::build(state);
	let http_listener = tokio::net::TcpListener::bind(config.bind_addr)
		.await
		.with_context(|| format!("failed to bind HTTP {}", config.bind_addr))?;
	tracing::info!(addr = %config.bind_addr, "HTTP API listening");
	let http = async move { axum::serve(http_listener, http_router).await.context("HTTP server error") };

	// ── gRPC (tonic) ──────────────────────────────────────────────────────────
	tracing::info!(addr = %config.grpc_addr, "gRPC API listening");
	let grpc = async move { grpc::serve(config.grpc_addr).await.context("gRPC server error") };

	// Run both forever; if either falls over, propagate and tear the other down.
	tokio::try_join!(http, grpc)?;
	Ok(())
}

fn init_tracing() {
	use tracing_subscriber::{EnvFilter, fmt, prelude::*};

	let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,backend=debug"));
	tracing_subscriber::registry().with(filter).with(fmt::layer()).with(sentry::integrations::tracing::layer()).init();
}
