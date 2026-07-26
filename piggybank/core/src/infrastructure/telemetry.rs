//! Observability adapter — the seam that hands captured errors to the monitoring
//! vendor via the shared `ev::error_monitoring` library (Sentry).
//!
//! The gRPC driving adapter calls [`report`] so the vendor can be swapped or
//! disabled without touching call sites (initialisation itself lives in `main`).
//! The integration is a no-op when Sentry has not been initialised (i.e.
//! `SENTRY_DSN` is unset).

use std::sync::atomic::{AtomicU64, Ordering};

/// The signer's sign-time unseal-failure message (the KEK-epoch dead-key class).
/// Must stay in sync with the signer's `Signer::unseal` wire message.
const UNSEAL_FAILURE_SIGNATURE: &str = "could not unseal the signing key";
/// Captures an unexpected error and forwards it to the error monitoring service.
///
/// Only call this for truly unexpected failures (5xx territory). Expected
/// domain errors — not found, validation, conflict — are client mistakes and
/// must not be reported here.
pub fn report(err: &dyn std::error::Error) {
	ev::error_monitoring::report(err);
}

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
/// Process-lifetime count of signer unseal failures observed on money-moving paths
/// (sweep gas/consolidation, withdrawal signing). Surfaced on `Readiness` → the admin
/// Overview, because each hit means funds are stranded on a dead-key address.
static UNSEAL_FAILURES: AtomicU64 = AtomicU64::new(0);

/// Process-lifetime count of reconciliation cash-invariant reads that timed out.
/// A transient blip is harmless (the next 60s scan usually succeeds), but a rising
/// count means the TigerBeetle bulk-`lookup_accounts` deadline is systematically
/// too tight or the ledger is overloaded — surfacing a drift early avoids days of
/// silent conservation-skip.
static CASH_INVARIANT_TIMEOUTS: AtomicU64 = AtomicU64::new(0);

/// Record a reconciliation cash-invariant read failure. Callers (the reconciliation
/// job) already log at ERROR; this counter lets a dashboard/alert distinguish a one-off
/// from a persistent failure without grep'ing logs.
pub fn note_cash_invariant_read_failure() {
	let total = CASH_INVARIANT_TIMEOUTS.fetch_add(1, Ordering::Relaxed) + 1;
	tracing::warn!(
		total,
		"reconciliation: cash-invariant read failed — TigerBeetle lookup_accounts unavailable. If this count climbs, the ledger may be overloaded or TB_CASH_INVARIANT_TIMEOUT may need raising."
	);
}

/// Total cash-invariant timeouts since process start. Surfaced on `Readiness` so the
/// admin Overview can flag a degraded reconciliation path.
pub fn cash_invariant_timeouts() -> u64 {
	CASH_INVARIANT_TIMEOUTS.load(Ordering::Relaxed)
}
