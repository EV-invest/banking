//! Shared domain crate.
//!
//! The single source of truth for domain types across the platform. The hub
//! server (`piggybank-core`) depends on it, and so do other service repos and
//! their wasm frontends (it stays wasm-safe). It never depends on the hub server
//! or any adapter.
//!
//! It carries the cross-cutting [`error::DomainError`], re-exports the `ev`
//! architecture building blocks, and holds the hub's bounded contexts — `auth` /
//! `authz` (identity + the RBAC matrix), `balance` / `money` (the chart of accounts
//! and the 18-dp USDT unit), `allocations` (the registry of investable products), `fees`
//! (the management + performance fee policy), and
//! the `users` / `subscriptions` / `redemptions` / `withdrawals` aggregates.

pub mod error;

pub mod allocations;
pub mod auth;
pub mod authz;
pub mod balance;
pub mod consilium;
pub mod fees;
pub mod money;
pub mod redemptions;
pub mod subscriptions;
pub mod users;
pub mod withdrawals;

/// Re-export of the `architecture` feature of the external `ev` crate — the
/// shared DDD tactical building blocks (`Id`, `Entity`, `AggregateRoot`,
/// `Repository`, `Gateway`, …) — so consumers reach them via
/// `domain::architecture::…` without depending on `ev` directly.
pub use ev::architecture;
