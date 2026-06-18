//! Observability seam — forwards unexpected auth failures to the shared
//! `ev::error_monitoring` library (Sentry).
//!
//! Auth runs in-process inside `piggybank-core`, which initialises Sentry for the
//! whole process; this seam reuses that client, so it is a no-op when Sentry is
//! unconfigured (and in consumers of this crate that never initialise it).
//!
//! Call [`report`] only for genuinely unexpected failures (5xx territory).
//! Expected auth outcomes — [`AuthError::InvalidToken`](crate::AuthError),
//! `UnknownKid`, `NotConfigured` — are client mistakes, not incidents.

/// Captures an unexpected error and forwards it to the error monitoring service.
pub fn report(err: &dyn std::error::Error) {
	ev::error_monitoring::report(err);
}
