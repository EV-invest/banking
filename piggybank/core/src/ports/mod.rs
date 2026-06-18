//! Driven ports — the outbound interfaces the application layer depends on,
//! implemented by `infrastructure`. The hexagonal "domain/port" layer.
//!
//! Built on the generic DDD building blocks in
//! [`domain::architecture`](domain::architecture):
//! - **`Repository` / `Reader`** leaf traits — one pair per aggregate, backed by
//!   Postgres and enrolled in a `UnitOfWork`. `Reader` is the CQRS read half.
//! - **`Ledger: Gateway`** — the TigerBeetle boundary. As a `Gateway` it can
//!   never join a `UnitOfWork` (the type system forbids it), so money operations
//!   stay outside the Postgres transaction by construction.
//!
//! Scaffold: intentionally empty — concrete leaf traits land per feature.
