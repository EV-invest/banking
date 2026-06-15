//! Shared domain crate.
//!
//! The single source of truth for domain types across the workspace; `backend`
//! and `frontend` depend on it, never on each other, and it stays wasm-safe.
//!
//! Scaffold: this seeds only the cross-cutting [`error::DomainError`] and
//! re-exports the `ev` architecture building blocks. Aggregates, value objects,
//! and ports are added under a `model` module as real features land.

pub mod error;

/// Re-export of the `architecture` feature of the external `ev` crate — the
/// shared DDD tactical building blocks (`Id`, `Entity`, `AggregateRoot`,
/// `Repository`, `Gateway`, `UnitOfWork`, …) — so consumers reach them via
/// `domain::architecture::…` without depending on `ev` directly.
pub use ev::architecture;
