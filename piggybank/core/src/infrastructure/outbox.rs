//! Transactional outbox (Postgres).
//!
//! Domain events drained from aggregates are inserted into an `outbox` table
//! **inside the same `UnitOfWork`** as the state change — the one place with real
//! ACID atomicity. A monotonic `seq` orders delivery; rows carry the event id for
//! idempotency. Drained by [`super::relay`].
//!
//! Scaffold: no table or repository yet (no migrations in the scaffold). The
//! `outbox` table + `OutboxRepository` land with the first write that spans more
//! than one aggregate or crosses into TigerBeetle.
