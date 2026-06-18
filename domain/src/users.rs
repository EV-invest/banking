//! `users` bounded context — investor accounts and their investments.
//!
//! Owns the aggregates for user accounts and the record of their capital
//! commitments into the bank. Identity links to the `auth` context; money links
//! to `balance`/`allocations` (authoritative in TigerBeetle).
//!
//! Scaffold: intentionally empty. Add aggregates, value objects, and ports here
//! as the feature lands.
