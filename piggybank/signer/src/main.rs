//! Composition root for the signer process.
//!
//! Loads config, **fail-fast** acquires the KEK (the signer refuses to start without
//! it), opens its own database, applies migrations, and serves the `signer.v1` gRPC
//! surface. A separate binary on purpose: the KEK and every plaintext key stay in
//! this address space, so a hub compromise can't move money.

use anyhow::Context;
use evbanking_contracts::signer::v1::signer_service_server::SignerServiceServer;
use piggybank_signer::{
	config::{SignerConfig, load_vault},
	secrets::WalletSecrets,
	service::Signer,
};
use sqlx::postgres::PgPoolOptions;
use tonic::transport::Server;

fn main() -> anyhow::Result<()> {
	dotenvy::dotenv().ok();
	init_tracing();

	tokio::runtime::Builder::new_multi_thread()
		.enable_all()
		.build()
		.context("failed to build tokio runtime")?
		.block_on(run())
}

async fn run() -> anyhow::Result<()> {
	let config = SignerConfig::from_env().context("failed to load signer configuration")?;
	// Acquire the KEK before anything else — no point opening sockets if we can't seal.
	let vault = load_vault()?;

	let pool = PgPoolOptions::new()
		.max_connections(5)
		.connect(&config.database_url)
		.await
		.context("failed to connect to the signer database")?;
	sqlx::migrate!().run(&pool).await.context("failed to apply signer migrations")?;

	let signer = Signer::new(vault, WalletSecrets::new(pool));

	tracing::info!(addr = %config.grpc_addr, "signer listening");
	Server::builder()
		.add_service(SignerServiceServer::new(signer))
		.serve(config.grpc_addr)
		.await
		.context("signer gRPC server error")?;
	Ok(())
}

fn init_tracing() {
	use tracing_subscriber::{EnvFilter, fmt, prelude::*};

	let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,piggybank_signer=debug"));
	tracing_subscriber::registry().with(filter).with(fmt::layer()).init();
}
