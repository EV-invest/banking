//! `balance` bounded context — company money.
//!
//! Owns the aggregates that track and govern the bank's own capital. Monetary
//! state is authoritative in TigerBeetle (the data plane); this context models
//! the metadata, identities, and invariants around it (the control plane), and
//! emits domain events that the hub server's outbox relays.
//!
//! Scaffold: intentionally empty. Add value objects (parse-don't-validate),
//! aggregates implementing [`architecture::AggregateRoot`](crate::architecture),
//! and `EmitsEvents` here as the feature lands.
