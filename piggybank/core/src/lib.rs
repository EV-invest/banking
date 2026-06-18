//! `piggybank-core` — the hub server library.
//!
//! The central bank's driving adapter is **gRPC** (tonic): the closed, internal
//! surface other services call. There is no HTTP here — browser/client traffic
//! reaches the hub through the `clients/core` BFF, which proxies HTTP↔gRPC.
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

use evbanking_auth::Authorizer;
use infrastructure::tigerbeetle::TigerBeetle;
use sqlx::PgPool;

pub mod application;
pub mod config;
pub mod infrastructure;
pub mod ports;
pub mod services;

/// Shared, cheaply-cloneable handles injected into the gRPC services. The
/// Postgres pool is the **control plane** (metadata, id-mapping, event
/// log/outbox, projections); TigerBeetle is the **data plane** (authoritative
/// money); the [`Authorizer`] verifies inbound requests via the in-process
/// channel to the auth task.
#[derive(Clone)]
pub struct AppState {
	pub pool: PgPool,
	pub tigerbeetle: Arc<TigerBeetle>,
	pub authorizer: Authorizer,
}

impl AppState {
	pub fn new(pool: PgPool, tigerbeetle: Arc<TigerBeetle>, authorizer: Authorizer) -> Self {
		Self { pool, tigerbeetle, authorizer }
	}
}
