//! Composition root for the whole `piggybank` system.
//!
//! Loads config, opens the driven infrastructure (Postgres control plane,
//! TigerBeetle ledger), then runs two in-process tasks that talk over the
//! [`Authorizer`](evbanking_auth::Authorizer) channel:
//!   - the **auth service** ([`evbanking_auth::AuthService`]) — issuance gRPC routes
//!     + the authorize channel, on `auth_grpc_addr`;
//!   - the **core** gRPC services (health/users/balance/funds/wallet) on `grpc_addr`,
//!     authorizing each request via the `Authorizer` core got from auth.

use std::{future::Future, sync::Arc, time::Duration};

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
		reaper::Reaper,
		reconciliation::Reconciliation,
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
use tokio_util::sync::CancellationToken;
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
	let pool = db::connect_sized(&config.database_url, config.db_max_connections)
		.await
		.context("failed to connect to the database")?;
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
	// It gets its own small pool so a burst of request traffic can't starve money
	// dispatch (and vice-versa) — the two planes no longer share the request pool.
	let relay_pool = db::connect_sized(&config.database_url, config.relay_db_max_connections)
		.await
		.context("failed to connect the relay's database pool")?;
	let relay_notify = Arc::new(Notify::new());
	let custody: Arc<dyn Custody> = Arc::new(StubCustody);
	let relay = Relay::new(relay_pool.clone(), ledger.clone(), custody, relay_notify.clone());

	// Recovery jobs, on the relay's dedicated pool so their periodic scans don't compete
	// with request traffic. Reconciliation watches the PG-vs-TB invariants and surfaces any
	// parked outbox row (TB wins, alert-only); the reaper owns the timeout for abandoned
	// sagas (alert on stuck `processing` withdrawals; auto-resolve the safe `queued` ones).
	let reconciliation = Reconciliation::new(relay_pool.clone(), ledger.clone());
	let reaper = Reaper::new(relay_pool, withdrawals.clone(), redemptions.clone(), relay_notify.clone());

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

	// Graceful shutdown, structured (no detached spawns): the core server, the auth task,
	// the provisioning loop, and the recovery jobs run as branches of one `join!` on this
	// task. ctrl_c / SIGTERM cancel a shared token; the gRPC servers `serve_with_shutdown`
	// on it (draining in-flight requests) and the relay finishes its current drain iteration
	// before exiting (already crash-safe between rows). Each branch *also* cancels the token
	// when it returns on its own (a signal, an error), and every branch exits once the token
	// is cancelled — so the first to finish triggers the rest to wind down on their own terms
	// rather than being aborted mid-work, and `join!` waits for them all.
	let shutdown = CancellationToken::new();
	let (signal, core, auth, provisioner, relay_done, reconciliation_done, reaper_done) = tokio::join!(
		await_signal(shutdown.clone()),
		branch(&shutdown, "core gRPC server", services::serve(config.grpc_addr, state, shutdown.clone().cancelled_owned())),
		branch(&shutdown, "auth service", auth_service.run(config.auth_grpc_addr, shutdown.clone().cancelled_owned())),
		branch(&shutdown, "provisioner", infallible(auth_sync::run_provisioner(provision_rx, users))),
		branch(&shutdown, "relay", infallible(relay.run(shutdown.clone()))),
		branch(&shutdown, "reconciliation", infallible(reconciliation.run(shutdown.clone()))),
		branch(&shutdown, "reaper", infallible(reaper.run(shutdown.clone()))),
	);
	let () = signal;
	// The first error (if any) becomes the process result; a clean shutdown is `Ok`.
	core.and(auth).and(provisioner).and(relay_done).and(reconciliation_done).and(reaper_done)
}

/// Run one composition-root branch to completion, mapping any error to `anyhow`, then cancel
/// the shared token so its peers start their graceful wind-down. It never returns early on a
/// peer's cancellation — the branch's own future owns its draining (the gRPC servers via
/// `serve_with_shutdown`, the loops via the token) — so `join!` over all branches drains them.
async fn branch<E: std::fmt::Display>(shutdown: &CancellationToken, name: &str, fut: impl Future<Output = Result<(), E>>) -> anyhow::Result<()> {
	let result = fut.await.map_err(|err| anyhow::anyhow!("{name} error: {err}"));
	if let Err(err) = &result {
		tracing::error!("{err}");
	}
	shutdown.cancel();
	result
}

/// Adapt an infallible loop task (`-> ()`) to the `Result` shape [`branch`] expects.
async fn infallible(fut: impl Future<Output = ()>) -> Result<(), std::convert::Infallible> {
	fut.await;
	Ok(())
}

/// Resolve on the first of `SIGINT` (ctrl_c), `SIGTERM`, or a peer cancelling `shutdown`,
/// then cancel the token. Racing the token keeps this a structured `join!` branch — it never
/// outlives the others. On non-Unix only ctrl_c exists; a failed handler registration is
/// logged and this branch idles on the token (the process can still be force-killed) rather
/// than tearing down at boot.
async fn await_signal(shutdown: CancellationToken) {
	#[cfg(unix)]
	{
		use tokio::signal::unix::{SignalKind, signal};
		match signal(SignalKind::terminate()) {
			Ok(mut term) => {
				tokio::select! {
					biased;
					() = shutdown.cancelled() => return,
					result = tokio::signal::ctrl_c() => {
						if let Err(err) = result {
							tracing::error!("failed to listen for ctrl_c: {err}");
							shutdown.cancelled().await;
							return;
						}
					},
					_ = term.recv() => {},
				}
			}
			Err(err) => {
				tracing::error!("failed to install SIGTERM handler: {err}");
				shutdown.cancelled().await;
				return;
			}
		}
	}
	#[cfg(not(unix))]
	tokio::select! {
		biased;
		() = shutdown.cancelled() => return,
		result = tokio::signal::ctrl_c() => {
			if let Err(err) = result {
				tracing::error!("failed to listen for ctrl_c: {err}");
				shutdown.cancelled().await;
				return;
			}
		},
	}
	tracing::info!("shutdown signal received — draining");
	shutdown.cancel();
}

fn init_tracing() {
	use tracing_subscriber::{EnvFilter, fmt, prelude::*};

	let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,piggybank_core=debug,evbanking_auth=debug"));
	tracing_subscriber::registry().with(filter).with(fmt::layer()).with(error_monitoring::tracing_layer()).init();
}
