//! Composition root for the whole `piggybank` system.
//!
//! Loads config, opens the driven infrastructure (Postgres control plane,
//! TigerBeetle ledger), then runs two in-process tasks that talk over the
//! [`Authorizer`](evbanking_auth::Authorizer) channel:
//!   - the **auth service** ([`evbanking_auth::AuthService`]) — issuance gRPC routes
//!     + the authorize channel, on `auth_grpc_addr`;
//!   - the **core** gRPC services (health/users/balance/funds/wallet) on `grpc_addr`,
//!     authorizing each request via the `Authorizer` core got from auth.

use std::{sync::Arc, time::Duration};

use anyhow::Context;
use ev::error_monitoring::{self, Config as SentryConfig};
use evbanking_auth::{AuthConfig, AuthService, provisioner_channel};
use evbanking_contracts::signer::v1::signer_service_client::SignerServiceClient;
use piggybank_core::{
	AppState,
	application::auth_sync,
	config::AppConfig,
	infrastructure::{
		custody::StubCustody,
		db,
		ledger::{self, TbLedger},
		nav::PgNav,
		positions::PgFundPositions,
		redemptions::PgRedemptions,
		relay::Relay,
		signer_addresses::SignerDepositAddresses,
		subscriptions::PgSubscriptions,
		tigerbeetle::TigerBeetle,
		users::PgUsers,
		withdrawals::PgWithdrawals,
	},
	ports::{Custody, DepositAddresses, FundPositionReader, NavRepository, RedemptionRepository, SubscriptionRepository, UserRepository, WithdrawalRepository, ledger::Ledger},
	services,
};
use tokio::sync::Notify;
use tonic::transport::Endpoint;

// Sentry must be initialised before the async runtime starts — no #[tokio::main].
fn main() -> anyhow::Result<()> {
	dotenvy::dotenv().ok();

	let config = AppConfig::from_env().context("failed to load configuration")?;

	// Guard must stay alive for the duration of main — dropping it flushes events.
	// `None` DSN → `init` returns `None`, so this binding is simply inert.
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

async fn run(config: AppConfig) -> anyhow::Result<()> {
	// ── driven infrastructure ─────────────────────────────────────────────────
	// The hub applies pending control-plane migrations on boot (idempotent). New
	// migration FILES are authored with the sqlx CLI (`sqlx migrate add …`), never
	// hand-written.
	let pool = db::connect(&config.database_url).await.context("failed to connect to the database")?;
	db::migrate(&pool).await.context("failed to apply database migrations")?;
	let tigerbeetle = Arc::new(TigerBeetle::connect(config.tigerbeetle_cluster_id, &config.tigerbeetle_address).context("failed to connect to TigerBeetle")?);

	// The data-plane money gateway over TigerBeetle (it also holds the pool for the
	// `tb_accounts` id-map — its own non-transactional concern). Seed the fund's
	// singleton accounts (a custody wallet + capital claim per network, plus the bank
	// mock) at boot; idempotent.
	let ledger: Arc<dyn Ledger> = Arc::new(TbLedger::new(tigerbeetle, pool.clone()));
	ledger::seed_singletons(ledger.as_ref()).await.context("failed to seed the fund's ledger accounts")?;

	// Product-analytics capture (native PostHog). A `None` key makes capture a
	// silent no-op, so this is safe to construct unconfigured.
	let analytics = ev::analytics::Analytics::new(config.posthog_key.clone(), config.posthog_host.clone());

	let users: Arc<dyn UserRepository> = Arc::new(PgUsers::new(pool.clone()));
	let withdrawals: Arc<dyn WithdrawalRepository> = Arc::new(PgWithdrawals::new(pool.clone()));
	let subscriptions: Arc<dyn SubscriptionRepository> = Arc::new(PgSubscriptions::new(pool.clone()));
	let redemptions: Arc<dyn RedemptionRepository> = Arc::new(PgRedemptions::new(pool.clone()));
	let nav: Arc<dyn NavRepository> = Arc::new(PgNav::new(pool.clone()));
	let positions: Arc<dyn FundPositionReader> = Arc::new(PgFundPositions::new(pool.clone()));

	// Deposit addresses are provisioned by the separate-process signer (it mints + seals
	// the keypair and returns the address; the hub never holds the key). Connect lazily so
	// the hub boots even if the signer starts after it — the first provision call is when
	// the signer must be reachable. Cached addresses are served from Postgres without it.
	let signer_channel = Endpoint::from_shared(config.signer_grpc_addr.clone())
		.context("SIGNER_GRPC_ADDR must be a valid URL, e.g. http://127.0.0.1:50053")?
		// Explicit deadlines so a half-open/stalled signer surfaces as a bounded error
		// (mapped to a retryable provisioning failure) instead of hanging the request.
		.connect_timeout(Duration::from_secs(3))
		.timeout(Duration::from_secs(10))
		.connect_lazy();
	let deposit_addresses: Arc<dyn DepositAddresses> = Arc::new(SignerDepositAddresses::new(pool.clone(), SignerServiceClient::new(signer_channel)));

	// The single-worker outbox relay moves money in TigerBeetle after each commit
	// (Write-Last); command handlers nudge it through `relay_notify` for low latency.
	// Custody is a separate trust domain — a stub stands in until the real signing
	// service exists; the relay broadcasts a withdrawal's on-chain leg through it.
	let relay_notify = Arc::new(Notify::new());
	let custody: Arc<dyn Custody> = Arc::new(StubCustody);
	let relay = Relay::new(pool.clone(), ledger.clone(), custody, relay_notify.clone());

	// ── auth service + user provisioning (in-process) ──────────────────────────
	// Auth owns the keys/JWKS and hands core an `Authorizer` (core → auth, verify);
	// core hands auth a `Provisioner` (auth → core, upsert users) and drains it.
	let auth_config = AuthConfig::from_env().context("failed to load auth configuration")?;
	let (provisioner, provision_rx) = provisioner_channel();
	let (auth_service, authorizer) = AuthService::try_new(auth_config, provisioner).context("failed to build the auth service")?;

	let state = AppState::new(
		pool,
		ledger,
		authorizer,
		analytics,
		users.clone(),
		withdrawals,
		subscriptions,
		redemptions,
		nav,
		positions,
		deposit_addresses,
		relay_notify,
		Arc::from(config.admin_subjects.clone()),
	);

	tracing::info!(core = %config.grpc_addr, auth = %config.auth_grpc_addr, "piggybank listening");

	// Structured concurrency: the core server, the auth task, and the provisioning
	// loop run as branches of one `select!` on this task — no detached spawns.
	// Whichever ends first (an error or shutdown) tears the process down.
	tokio::select! {
		result = services::serve(config.grpc_addr, state) => result.context("core gRPC server error")?,
		result = auth_service.run(config.auth_grpc_addr) => result.context("auth service error")?,
		() = auth_sync::run_provisioner(provision_rx, users) => {},
		() = relay.run() => {},
	}
	Ok(())
}

fn init_tracing() {
	use tracing_subscriber::{EnvFilter, fmt, prelude::*};

	let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,piggybank_core=debug,evbanking_auth=debug"));
	tracing_subscriber::registry().with(filter).with(fmt::layer()).with(error_monitoring::tracing_layer()).init();
}
