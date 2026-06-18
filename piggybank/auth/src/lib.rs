//! The hub's auth — a service **and** a shared verification flow.
//!
//! Two facets, one crate:
//!
//! 1. **The auth service** ([`service`], [`authorizer`]). Runs as its own task
//!    inside `piggybank-core`. It owns the signing keys / JWKS / refresh store,
//!    serves the **issuance** gRPC routes (exchange a client login for the hub's
//!    own JWT, refresh, JWKS), and answers in-process authorize requests from
//!    core over an [`Authorizer`] channel — so core authorizes gRPC requests by
//!    talking to auth across a task boundary, not over the network.
//!
//! 2. **The verification flow** ([`jwks::verify_token`], [`interceptor`]). What
//!    *other* service repos import: stateless local verification of the hub's
//!    JWTs against cached JWKS public keys — **no per-request round trip, no
//!    per-service token storage** (a per-service Redis denylist is an anti-pattern).
//!
//! Design (see `docs/ARCHITECTURE.md`): access tokens are short-lived asymmetric
//! JWTs (EdDSA/RS256); revocation is short TTLs + refresh rotation at the central
//! service, plus an optional `token_version` claim checked locally.
//!
//! This crate is **wasm-unsafe** (crypto backend + tonic), so it must never be a
//! dependency of the wasm-safe `domain` crate.
//!
//! Scaffold: types and entry points are in place; bodies return
//! [`AuthError::NotConfigured`] until the auth feature lands.

pub mod authorizer;
pub mod claims;
pub mod interceptor;
pub mod jwks;
pub mod service;

pub use authorizer::Authorizer;
pub use claims::Claims;
pub use jwks::{JwksCache, verify_token};
pub use service::AuthService;
use thiserror::Error;

/// Errors surfaced by the auth flow.
#[derive(Debug, Error)]
pub enum AuthError {
	/// The flow has not been wired yet (scaffold state).
	#[error("auth flow is not configured")]
	NotConfigured,
	/// The auth service task could not be reached — its channel is closed because
	/// the task is gone or never started. Distinct from [`NotConfigured`]: the flow
	/// may be wired, the service is just unreachable (maps to gRPC `UNAVAILABLE`).
	#[error("auth service unavailable")]
	Unavailable,
	/// The token is malformed, expired, or fails signature/claim validation.
	#[error("invalid or expired token")]
	InvalidToken,
	/// No cached JWKS public key matches the token's `kid` header.
	#[error("unknown signing key: {0}")]
	UnknownKid(String),
}
