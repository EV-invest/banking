//! Observability adapter — the seam that hands captured errors to the monitoring
//! vendor via the shared `ev::error_monitoring` library (Sentry).
//!
//! The gRPC driving adapter calls [`report`] so the vendor can be swapped or
//! disabled without touching call sites (initialisation itself lives in `main`).
//! The integration is a no-op when Sentry has not been initialised (i.e.
//! `SENTRY_DSN` is unset).

use std::sync::atomic::{AtomicU64, Ordering};

/// Captures an unexpected error and forwards it to the error monitoring service.
///
/// Only call this for truly unexpected failures (5xx territory). Expected
/// domain errors — not found, validation, conflict — are client mistakes and
/// must not be reported here.
pub fn report(err: &dyn std::error::Error) {
	ev::error_monitoring::report(err);
}

/// The signer's sign-time unseal-failure message (the KEK-epoch dead-key class).
/// Must stay in sync with the signer's `Signer::unseal` wire message.
const UNSEAL_FAILURE_SIGNATURE: &str = "could not unseal the signing key";

/// Process-lifetime count of signer unseal failures observed on money-moving paths
/// (sweep gas/consolidation, withdrawal signing). Surfaced on `Readiness` → the admin
/// Overview, because each hit means funds are stranded on a dead-key address.
static UNSEAL_FAILURES: AtomicU64 = AtomicU64::new(0);

pub fn unseal_failures() -> u64 {
	UNSEAL_FAILURES.load(Ordering::Relaxed)
}

/// Classify a signer-seam error: a dead-key unseal failure is counted and logged at
/// ERROR — it means real funds already cannot move, so it must never scroll by as a
/// WARN retry loop. Returns whether the message matched (callers keep their own
/// error handling either way).
pub fn note_signer_error(op: &'static str, wallet: &str, message: &str) -> bool {
	if !message.contains(UNSEAL_FAILURE_SIGNATURE) {
		return false;
	}
	let total = UNSEAL_FAILURES.fetch_add(1, Ordering::Relaxed) + 1;
	tracing::error!(
		op,
		%wallet,
		total,
		"signer could not unseal the signing key — PROVABLY DEAD KEY (KEK epoch): funds on this wallet's address cannot move. Check signer GetKeyHealth; supersede via RotateDepositAddress"
	);
	true
}
