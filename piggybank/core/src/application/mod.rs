//! Application layer — use cases, split CQRS-style.
//!
//! - **Command handlers** (write side) open one [`UnitOfWork`] (a single Postgres
//!   transaction), mutate aggregates, and drain their `EmitsEvents` into the
//!   event log + outbox in that same transaction. Money is moved in TigerBeetle
//!   **after** the Postgres commit (Write-Last), via the outbox relay/sagas.
//! - **Query handlers** (read side) depend only on the narrow `Reader` ports and
//!   read Postgres projections; authoritative balances come from TigerBeetle.
//!
//! Both depend on the ports in [`crate::ports`], never on concrete adapters.
//!
//! Scaffold: intentionally empty — handlers land per feature, no business logic.
