//! `allocations` bounded context — distribution of capital inside the bank.
//!
//! Owns the aggregates governing how company and user capital is allocated
//! across the bank's strategies and external services. An allocation move is a
//! cross-boundary saga: intent + events recorded in Postgres, money moved in
//! TigerBeetle (two-phase pending transfers), reconciled afterwards.
//!
//! Scaffold: intentionally empty. Add aggregates, value objects, and the saga's
//! domain events here as the feature lands.
