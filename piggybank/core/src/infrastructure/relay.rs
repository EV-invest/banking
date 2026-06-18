//! Outbox relay — the dispatcher task.
//!
//! Polls unsent [`outbox`](super::outbox) rows (`FOR UPDATE SKIP LOCKED`, ordered
//! by `seq`), then drives side effects: publishes domain events to projections
//! and issues TigerBeetle transfers (money written **last**, after Postgres has
//! committed the intent). Delivery is at-least-once, so consumers must be
//! idempotent — TB transfer ids are derived deterministically from the event id
//! (a duplicate submit returns `exists`), and projections upsert by event id.
//!
//! Polling now; the upgrade path is logical-replication CDC behind the same
//! table (no schema change).
//!
//! Scaffold: documented placeholder — the relay task spawns alongside the gRPC
//! server once the outbox lands.
