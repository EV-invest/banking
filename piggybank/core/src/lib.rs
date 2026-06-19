#![feature(default_field_values)]
//! `piggybank-core` — the hub server library.
//!
//! The central bank's driving adapter is **gRPC** (tonic): the closed, internal
//! surface other services call. There is no HTTP here — browser/client traffic
//! reaches the hub through the `clients/cabinet` BFF, which proxies HTTP↔gRPC.
//!
//! `main` is the composition root for the whole `piggybank` system: it runs the
//! core gRPC services **and** the [`evbanking_auth`] auth service as separate
//! in-process tasks. Auth hands core an [`Authorizer`] — a channel to the auth
//! task — so core authorizes incoming gRPC requests across a task boundary
//! instead of over the network.
//!
//! Hexagonal layout over the shared `domain` — each module is named for its role,
//! not its transport:
//!   services        — driving adapter (the hub's gRPC service surface; tonic-web)
//!   application     — use cases (CQRS command/query handlers)
//!   ports           — driven ports (Repository/Reader leaf traits, Ledger gateway)
//!   infrastructure  — driven adapters (Postgres control plane, TigerBeetle ledger,
//!                     telemetry)
//!
//! Scaffold: the application/ports layers are placeholders; no business logic or
//! migrations land until a feature explicitly asks.

use std::sync::Arc;

use ev::analytics::Analytics;
use evbanking_auth::Authorizer;
use ports::{AllocationRepository, UserRepository, ledger::Ledger};
use sqlx::PgPool;
use tokio::sync::Notify;

pub mod application;
pub mod config;
pub mod infrastructure;
pub mod ports;
pub mod services;

/// Shared, cheaply-cloneable handles injected into the gRPC services. The
/// Postgres pool is the **control plane** (metadata, id-mapping, event
/// log/outbox, projections); the [`Ledger`] gateway is the **data plane**
/// (authoritative money in TigerBeetle); the [`Authorizer`] verifies inbound
/// requests via the in-process channel to the auth task; [`Analytics`] is the
/// product-analytics seam (native PostHog capture, a no-op until `POSTHOG_KEY` is
/// set). Command handlers `notify` the [`relay_notify`](AppState::relay_notify)
/// after a commit so the outbox relay moves money promptly.
#[derive(Clone)]
pub struct AppState {
	pub pool: PgPool,
	/// The TigerBeetle money gateway (data plane).
	pub ledger: Arc<dyn Ledger>,
	pub authorizer: Authorizer,
	pub analytics: Analytics,
	/// The `users` aggregate's driven port (Postgres control plane).
	pub users: Arc<dyn UserRepository>,
	/// The `allocations` aggregate's driven port (Postgres control plane).
	pub allocations: Arc<dyn AllocationRepository>,
	/// Nudges the outbox relay to dispatch right after a command commits.
	pub relay_notify: Arc<Notify>,
	/// User ids permitted to call admin RPCs (config allowlist; see [`config`]).
	pub admin_subjects: Arc<[String]>,
}

impl AppState {
	#[allow(clippy::too_many_arguments)]
	pub fn new(
		pool: PgPool,
		ledger: Arc<dyn Ledger>,
		authorizer: Authorizer,
		analytics: Analytics,
		users: Arc<dyn UserRepository>,
		allocations: Arc<dyn AllocationRepository>,
		relay_notify: Arc<Notify>,
		admin_subjects: Arc<[String]>,
	) -> Self {
		Self {
			pool,
			ledger,
			authorizer,
			analytics,
			users,
			allocations,
			relay_notify,
			admin_subjects,
		}
	}

	/// Whether `subject` (a token `sub`) is on the admin allowlist.
	pub fn is_admin(&self, subject: &str) -> bool {
		self.admin_subjects.iter().any(|s| s == subject)
	}
}
