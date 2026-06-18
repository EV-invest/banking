//! tonic request interceptor — the choke point every service mounts to authorize
//! inbound gRPC calls.
//!
//! It pulls the bearer token from request metadata, verifies it locally with
//! [`verify_token`](crate::jwks::verify_token) against the cached JWKS, and (when
//! the feature lands) injects the validated [`Claims`](crate::Claims) into the
//! request extensions for handlers to read.
//!
//! The real implementation is an **async** interceptor (so an unknown-`kid` can
//! trigger a JWKS refresh); this scaffold is a synchronous passthrough.

use tonic::{Request, Status};

/// Authorize an inbound request. Scaffold: passthrough — no token is required
/// yet. Wire `verify_token` here per `docs/ARCHITECTURE.md`.
///
/// `Result<_, Status>` is tonic's mandated interceptor signature; `Status` is a
/// large type we don't control, so the large-err lint doesn't apply.
#[allow(clippy::result_large_err)]
pub fn authorize<T>(request: Request<T>) -> Result<Request<T>, Status> {
	Ok(request)
}
