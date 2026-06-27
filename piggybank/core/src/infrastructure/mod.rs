//! Infrastructure: driven adapters over the concrete external systems the hub
//! runs on.
//!
//! - [`db`] — Postgres **control plane**: pool and migrations.
//! - [`tigerbeetle`] — the connected TigerBeetle client.
//! - [`ledger`] — the **data plane** `Ledger` [`Gateway`](domain::architecture::Gateway)
//!   over TigerBeetle (the chart of accounts, transfers, two-phase saga ops).
//! - [`users`] — Postgres repository for the `User` aggregate;
//!   [`subscriptions`] / [`redemptions`] / [`withdrawals`] — repositories for the
//!   money-plane aggregates (atomic state + drained events).
//! - [`outbox`] — the transactional outbox written inside the same transaction as
//!   the state change, plus its drain side.
//! - [`relay`] — the single-worker saga dispatcher that drains the outbox and
//!   issues TigerBeetle transfers (Write-Last), idempotently.
//! - [`reconciliation`] — the periodic PG-vs-TB discrepancy scan (cash invariant,
//!   clearing vs in-flight withdrawals, parked-row surface); alert-only, TB wins.
//! - [`reaper`] — the abandoned-saga sweep: alerts on stuck `processing` withdrawals
//!   and auto-resolves the safe `queued` redemptions/withdrawals past a max age.
//! - [`telemetry`] — the observability adapter: the one seam that hands errors to
//!   the monitoring vendor, so call sites stay vendor-agnostic.

pub mod bridge;
pub mod bsc_rpc;
pub mod custody;
pub mod db;
pub mod deposit_watcher;
pub mod ledger;
pub mod nav;
pub mod outbox;
pub mod positions;
pub mod reaper;
pub mod reconciliation;
pub mod redemptions;
pub mod relay;
pub mod signer_addresses;
pub mod subscriptions;
pub mod sweep;
pub mod telemetry;
pub mod tigerbeetle;
pub mod users;
pub mod withdrawal_watcher;
pub mod withdrawals;
