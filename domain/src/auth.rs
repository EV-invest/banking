//! `auth` bounded context — identities (wasm-safe half).
//!
//! Owns the pure, transport-free identity types shared across the platform
//! (user/service principal identities, claim shapes). The *server-only* token
//! machinery — JWKS, signing, verification, the tonic interceptor — lives in the
//! `evbanking_auth` crate, which is wasm-unsafe and therefore must NOT be a
//! dependency of this crate. Keep this module free of crypto and I/O so `domain`
//! stays wasm-safe for service frontends.
//!
//! Scaffold: intentionally empty. Add identity value objects and claim types
//! here as the feature lands.
