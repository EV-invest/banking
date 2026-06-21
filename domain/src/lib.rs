#![feature(default_field_values)]
//! Shared domain crate.
//!
//! The single source of truth for domain types across the platform. The hub
//! server (`piggybank-core`) depends on it, and so do other service repos and
//! their wasm frontends (it stays wasm-safe). It never depends on the hub server
//! or any adapter.
//!
//! Scaffold: this seeds the cross-cutting [`error::DomainError`], re-exports the
//! `ev` architecture building blocks, and declares the four bounded contexts the
//! hub owns. Each context module is an empty placeholder — aggregates, value
//! objects, and ports land there as real features arrive.

pub mod error;

pub mod auth;
pub mod balance;
pub mod money;
pub mod redemptions;
pub mod subscriptions;
pub mod users;
pub mod withdrawals;

/// Re-export of the `architecture` feature of the external `ev` crate — the
/// shared DDD tactical building blocks (`Id`, `Entity`, `AggregateRoot`,
/// `Repository`, `Gateway`, `UnitOfWork`, …) — so consumers reach them via
/// `domain::architecture::…` without depending on `ev` directly.
pub use ev::architecture;
