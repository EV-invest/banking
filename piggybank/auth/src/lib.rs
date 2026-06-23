#![feature(default_field_values)]
//! The hub's auth — a service **and** a shared verification flow.
//!
//! Two facets, one crate. Pick the half you need:
//!
//! # For the hub (`piggybank-core`)
//!
//! - [`AuthService`] runs as its own task inside the composition root. It owns the
//!   signing keys / JWKS / Google client / refresh store, serves the **issuance**
//!   gRPC routes (`Exchange`/`Refresh`/`Logout`/`Jwks`), provisions users in
//!   process over the [`Provisioner`] channel (auth → core), and answers core's
//!   authorize requests over the [`Authorizer`] channel (core → auth). Both
//!   channels cross a task boundary, never the network.
//! - Core mounts [`grpc_auth_layer`]`(authorizer)` on each data service to
//!   authorize inbound gRPC; handlers read the verified [`Claims`] with
//!   [`claims_of`].
//!
//! # For a downstream service (a separate repo)
//!
//! - Depend on `evbanking_contracts` (the gRPC stubs) and this crate.
//! - Build a [`Verifier`] from [`VerifierConfig`] and mount
//!   [`grpc_auth_layer`]`(verifier)` — it verifies the hub's tokens **locally**
//!   against the cached JWKS (no per-request round trip, no per-service token
//!   storage; a per-service denylist is an anti-pattern).
//! - Authenticate your own onward calls into the hub with a
//!   [`ServiceTokenSource`] (a `typ=service`, distinct-`aud` token).
//!
//! Tokens are short-TTL asymmetric JWTs (EdDSA/Ed25519); revocation is short TTLs
//! with refresh rotation, plus a `token_version` claim enforced at refresh. See
//! `docs/ARCHITECTURE.md` and `piggybank/auth/README.md`.
//!
//! This crate is **wasm-unsafe** (crypto backend + tonic + reqwest), so it must
//! never be a dependency of the wasm-safe `domain` crate.

pub mod authorizer;
pub mod claims;
pub mod config;
pub mod interceptor;
pub mod jwks;
pub mod provisioner;
pub mod service;
pub mod service_token;
pub mod telemetry;
pub mod verifier;

// Issuance internals — host-only (used by `service` via `crate::`), not part of the
// verify-side surface downstream service repos import, so kept private.
mod google;
mod management;
mod signer;

pub use authorizer::Authorizer;
pub use claims::{Claims, TokenType};
pub use config::{AuthConfig, GoogleConfig, SigningConfig, VerifierConfig};
pub use interceptor::{AuthLayer, Authenticate, claims_of, grpc_auth_layer};
pub use jwks::{JwksCache, VerifyPolicy, verify_token};
pub use provisioner::{ProvisionCommand, ProvisionRequest, ProvisionedUser, Provisioner, provisioner_channel};
pub use service::AuthService;
pub use service_token::ServiceTokenSource;
use thiserror::Error;
pub use verifier::Verifier;

/// Errors surfaced by the auth flow.
#[derive(Debug, Error)]
pub enum AuthError {
	/// The flow has not been wired yet (no signing key configured — dev/CI).
	#[error("auth flow is not configured")]
	NotConfigured,
	/// An in-process auth task (authorize or provision) could not be reached — its
	/// channel is closed. The flow may be wired; the task is just gone.
	#[error("auth service unavailable")]
	Unavailable,
	/// No bearer token was presented.
	#[error("missing bearer token")]
	MissingToken,
	/// The token is malformed, expired, or fails signature/claim validation
	/// (including a wrong audience or token type for this verifier).
	#[error("invalid or expired token")]
	InvalidToken,
	/// No cached JWKS public key matches the token's `kid` header.
	#[error("unknown signing key: {0}")]
	UnknownKid(String),
	/// The upstream identity provider (Google) rejected the exchange or returned an
	/// unverifiable assertion.
	#[error("identity provider error: {0}")]
	Provider(String),
	/// The JWKS could not be refreshed from the hub.
	#[error("jwks refresh failed: {0}")]
	JwksFetch(String),
}

impl AuthError {
	/// Whether this is an operational incident worth reporting (5xx territory),
	/// versus an expected client/dev outcome.
	pub fn is_unexpected(&self) -> bool {
		matches!(self, Self::Unavailable | Self::JwksFetch(_))
	}
}

impl From<&AuthError> for tonic::Status {
	fn from(err: &AuthError) -> Self {
		use AuthError::*;
		match err {
			MissingToken => tonic::Status::unauthenticated("missing bearer token"),
			InvalidToken => tonic::Status::unauthenticated("invalid or expired token"),
			UnknownKid(_) => tonic::Status::unauthenticated("unknown signing key"),
			Provider(_) => tonic::Status::unauthenticated("identity provider rejected the request"),
			NotConfigured => tonic::Status::unavailable("auth not configured"),
			Unavailable => tonic::Status::unavailable("auth service unavailable"),
			JwksFetch(_) => tonic::Status::unavailable("could not refresh signing keys"),
		}
	}
}

impl From<AuthError> for tonic::Status {
	fn from(err: AuthError) -> Self {
		(&err).into()
	}
}
