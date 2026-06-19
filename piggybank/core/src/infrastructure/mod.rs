//! Infrastructure: driven adapters over the concrete external systems the hub
//! runs on.
//!
//! - [`db`] — Postgres **control plane**: pool, `UnitOfWork` (one transaction),
//!   repositories, the domain event log, and CQRS projections.
//! - [`tigerbeetle`] — the **data plane** `Ledger` gateway (authoritative money).
//! - [`outbox`] — the transactional outbox written inside the same `UnitOfWork`
//!   as state changes.
//! - [`relay`] — the dispatcher that drains the outbox: publishes events and
//!   issues TigerBeetle transfers (Write-Last), idempotently.
//! - [`telemetry`] — the observability adapter: the one seam that hands errors to
//!   the monitoring vendor, so call sites stay vendor-agnostic.
//!
//! Scaffold: `db`/`tigerbeetle` open a client/pool; `outbox`/`relay` are
//! documented placeholders. Domain mapping lands on top as features arrive.

pub mod db;
pub mod outbox;
pub mod relay;
pub mod telemetry;
pub mod tigerbeetle;
pub mod users;
