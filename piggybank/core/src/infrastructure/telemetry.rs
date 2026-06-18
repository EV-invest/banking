//! Observability adapter — the seam that hands captured errors to the monitoring
//! vendor (Sentry).
//!
//! The gRPC driving adapter calls [`report`] so the vendor can be swapped or
//! disabled without touching call sites (initialisation itself lives in `main`).
//! The integration is a no-op when Sentry has not been initialised (i.e.
//! `SENTRY_DSN` is unset).

/// Captures an unexpected error and forwards it to the error monitoring service.
///
/// Only call this for truly unexpected failures (5xx territory). Expected
/// domain errors — not found, validation, conflict — are client mistakes and
/// must not be reported here.
pub fn report(err: &dyn std::error::Error) {
	sentry::capture_error(err);
}
