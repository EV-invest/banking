//! Composition root for the signer process.
//!
//! Loads config, **fail-fast** acquires the KEK (the signer refuses to start without
//! it), opens its own database, applies migrations, and serves the `signer.v1` gRPC
//! surface. A separate binary on purpose: the KEK and every plaintext key stay in
//! this address space, so a hub compromise can't move money.

use std::sync::Arc;

use color_eyre::eyre::Context;
use evbanking_auth::{Verifier, grpc_auth_layer};
use evbanking_contracts::signer::v1::signer_service_server::SignerServiceServer;
use piggybank_signer::{
	backend::{CustodyMinter, KeyBackend, LocalVault},
	config::{KeyBackendKind, SignerConfig, TlsConfig, load_vault},
	kek_guard,
	policy::SignerPolicy,
	secrets::WalletSecrets,
	service::Signer,
	spend_brake::SpendBrakeStore,
	turnkey::TurnkeyBackend,
};
use sqlx::postgres::PgPoolOptions;
use tonic::transport::{Certificate, Identity, Server, ServerTlsConfig};
use tower::Layer;

fn main() -> color_eyre::Result<()> {
	color_eyre::install()?;
	dotenvy::dotenv().ok();
	// Held for the process lifetime — dropping flushes OTel logs/traces.
	let _otel_guard = init_tracing();

	tokio::runtime::Builder::new_multi_thread()
		.enable_all()
		.build()
		.context("failed to build tokio runtime")?
		.block_on(run())
}

async fn run() -> color_eyre::Result<()> {
	let config = SignerConfig::from_env().context("failed to load signer configuration")?;
	// Acquire the KEK before anything else — no point opening sockets if we can't seal.
	let vault = load_vault()?;

	let pool = PgPoolOptions::new()
		.max_connections(5)
		.connect(&config.database_url)
		.await
		.context("failed to connect to the signer database")?;
	sqlx::migrate!().run(&pool).await.context("failed to apply signer migrations")?;

	// The pool is shared: the secrets store owns one handle, the boot-time brake read below
	// takes the other (`Signer` builds its own stores over the secrets' pool).
	let secrets = WalletSecrets::new(pool.clone());
	// KEK-epoch guard BEFORE any RPC can be served: a wrong-KEK boot dies here (loudly)
	// instead of minting keys whose funds could never move. Per-row casualties are
	// reported inside (ERROR) and via the GetKeyHealth diagnostics.
	kek_guard::enforce(&vault, &secrets).await.context("KEK epoch guard refused to serve")?;

	// The signer's independent spend policy — the second gate that holds even if the hub is
	// compromised. The per-class rules (sweep → treasury only, gas station → own addresses
	// only, deposit native never), the fee budget, the top-up cap, the treasury USDT caps and
	// the spend windows are always on; only the allowlist is a no-op until an operator sets it.
	let policy = SignerPolicy::from_env().context("failed to load signer spend policy")?;
	tracing::info!(
		fee_budget = ?policy.fee_budget(),
		gas_topup_caps = ?policy.gas_topup(),
		token_pins = ?policy.token_pins(),
		max_transfer_usdt = policy.max_transfer_usdt(),
		treasury_usdt_per_hour = policy.treasury_usdt_per_hour(),
		allowlisted_destinations = policy.allowlist_len(),
		treasury_native_allowlisted_destinations = policy.treasury_native_allowlist_len(),
		treasury_jetton_wallet_pinned = policy.treasury_jetton_wallet_pinned(),
		treasury_native = ?policy.treasury_native(),
		native_spend_per_hour = ?policy.native_spend(),
		tron_signing_enabled = policy.tron_signing_enabled(),
		"signer spend policy active: sweeps only to the treasury, gas top-ups only to own addresses, no native out of deposit wallets, treasury native off unless opted in, treasury USDT capped per payout and per hour, native spend windowed per wallet, jetton wallets pinned on first use"
	);
	if !policy.treasury_jetton_wallet_pinned() {
		tracing::warn!("treasury jetton wallet will be pinned on first use — set SIGNER_TON_TREASURY_JETTON_WALLET");
	}
	// The operator's brake as it stands at boot — read again on every signing request, but
	// a signer that cannot read it now would refuse everything, so say so before listening.
	let brake = SpendBrakeStore::new(pool).read().await.context("failed to read the signer's spend brake")?;
	if brake.halted() {
		tracing::warn!(
			halted = brake.halted(),
			max_transfer_usdt = ?brake.max_transfer_usdt(),
			max_treasury_usdt_per_hour = ?brake.max_treasury_usdt_per_hour(),
			native_spend = ?brake.native_spend(),
			reason = ?brake.reason(),
			updated_at = %brake.updated_at(),
			"signer spend brake state: HALTED — every signature is refused until an operator releases it"
		);
	} else {
		tracing::info!(
			halted = brake.halted(),
			max_transfer_usdt = ?brake.max_transfer_usdt(),
			max_treasury_usdt_per_hour = ?brake.max_treasury_usdt_per_hour(),
			native_spend = ?brake.native_spend(),
			reason = ?brake.reason(),
			updated_at = %brake.updated_at(),
			"signer spend brake state"
		);
	}

	// Where NEW keys are minted and existing ones signed. `local` is the default and the
	// rollback: flipping KEY_BACKEND back restores the previous behaviour with no data change,
	// which is what makes each migration phase reversible. The KEK stays loaded either way —
	// every `backend='local'` row still needs it (phase 5 is what removes it).
	let vault = Arc::new(vault);
	// The custodian half is `Some` only under `KEY_BACKEND=turnkey`, and it is what the
	// phase-4 `MigrateAddressToCustodian` path mints through. One `Arc<TurnkeyBackend>` serves
	// both roles, so the migration can never mint through a different client (or a different
	// derivation) than ordinary provisioning does.
	let (backend, custodian): (Arc<dyn KeyBackend>, Option<Arc<dyn CustodyMinter>>) = match config.key_backend {
		KeyBackendKind::Local => (Arc::new(LocalVault::new(Arc::clone(&vault), secrets.clone())), None),
		// Fail-fast: an operator who asked for the custodian must not get a silent local boot.
		KeyBackendKind::Turnkey => {
			let turnkey = Arc::new(TurnkeyBackend::from_env(secrets.clone()).context("failed to build the Turnkey key backend")?);
			(Arc::clone(&turnkey) as Arc<dyn KeyBackend>, Some(turnkey as Arc<dyn CustodyMinter>))
		}
	};
	tracing::info!(key_backend = ?config.key_backend, "signer key backend selected");

	let signer = Signer::with_backend(backend, custodian, vault, secrets, policy);

	// Authenticate the seam: a stateless verifier accepts only the hub's service token
	// (verified against the auth service's JWKS). Mounted as the choke point in front of
	// the service, so an unauthenticated or wrong-audience/type call is rejected with
	// UNAUTHENTICATED before it reaches `provision_address`. A lazy verifier means the
	// signer still boots if auth starts after it; the first call is when auth must be up.
	let auth = grpc_auth_layer(Verifier::try_new(config.verifier).context("failed to build the signer's service-token verifier")?);

	let mut server = Server::builder();
	if let Some(tls) = config.tls {
		server = server.tls_config(server_tls(&tls)?).context("failed to configure signer TLS")?;
	}

	tracing::info!(addr = %config.grpc_addr, "signer listening");
	server
		.add_service(auth.layer(SignerServiceServer::new(signer)))
		.serve(config.grpc_addr)
		.await
		.context("signer gRPC server error")?;
	Ok(())
}

/// Build the server TLS config from the loaded PEM material; a client-CA root upgrades
/// it to mTLS so only the hub's pinned client certificate is accepted on the seam.
fn server_tls(tls: &TlsConfig) -> color_eyre::Result<ServerTlsConfig> {
	let identity = Identity::from_pem(&tls.cert_pem, &tls.key_pem);
	let mut config = ServerTlsConfig::new().identity(identity);
	if let Some(ca) = &tls.client_ca_pem {
		config = config.client_ca_root(Certificate::from_pem(ca));
	}
	Ok(config)
}

// Returns the OTel guard (flushes/shuts down on drop); bind it in `main`. `None`
// when OTEL_EXPORTER_OTLP_ENDPOINT is unset — the layers are then inert. Runs
// before config load, so it reads APP_ENV directly for the sampling policy.
fn init_tracing() -> Option<ev::otel::Telemetry> {
	use tracing_subscriber::{EnvFilter, fmt, prelude::*};

	let environment = std::env::var("APP_ENV").unwrap_or_else(|_| "development".to_string());
	let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,piggybank_signer=debug"));
	let (otel_guard, otel_layers) = ev::otel::telemetry(&ev::otel::Config {
		environment: environment.clone(),
		traces_sample_rate: ev::otel::Config::traces_sample_rate_for(&environment),
	})
	.unzip();
	tracing_subscriber::registry().with(filter).with(fmt::layer().json()).with(otel_layers).init();
	otel_guard
}
