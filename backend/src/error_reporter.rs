//! Error reporting — the only place in the codebase that calls Sentry directly.
//!
//! Both driving adapters (HTTP `api`, gRPC `grpc`) call [`report`] so the vendor
//! can be swapped or disabled without touching call sites. The integration is a
//! no-op when Sentry has not been initialised (i.e. `SENTRY_DSN` is unset).

/// Captures an unexpected error and forwards it to the error monitoring service.
///
/// Only call this for truly unexpected failures (5xx territory). Expected
/// domain errors — not found, validation, conflict — are client mistakes and
/// must not be reported here.
pub fn report(err: &dyn std::error::Error) {
	sentry::capture_error(err);
}
