//! Reconciliation — the PG-vs-TB discrepancy scan (TB always wins).
//!
//! The outbox relay can **park** a money-moving event into a distinct terminal state
//! (`outbox.parked_at`, see [`super::outbox`]) when it hits a non-retryable failure. A
//! parked event is no longer dropped, but it still needs a watcher: this periodic job is
//! the design's reconciliation seam (PATTERNS.md § Reconciliation). It asserts three
//! things and **alerts** (a Sentry-shipped `error!`) on any drift, without mutating money
//! — TB is authoritative, so recovery is an operator/treasury action, not an auto-write:
//!
//!   1. **Global cash invariant** — `sum(custody) == sum(claims)` on the USDT ledger,
//!      read straight from TigerBeetle ([`Ledger::cash_invariant`]).
//!   2. **Clearing vs control-plane** — the `clearing` account's reserved (pending +
//!      posted) balance vs the gross of every `queued`/`processing` withdrawal in
//!      Postgres; a mismatch means a withdrawal whose reserve parked (nothing locked) or a
//!      stranded reservation.
//!   3. **Parked-row scan** — every `outbox.parked_at` row, surfaced with its
//!      `last_error` and whether it has been flagged for compensation.
//!
//! It is **read-only** and idempotent, so running it on every standby is harmless; it is
//! wired as one `select!` branch of the composition root next to the relay.

use std::{sync::Arc, time::Duration};

use domain::balance::LedgerAccountKey;
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::{infrastructure::outbox, ports::ledger::Ledger};

/// How often the reconciliation scan runs. Discrepancies are rare and operator-resolved,
/// so a slow cadence is fine; the relay (sub-second) owns the hot path.
const SCAN_INTERVAL: Duration = Duration::from_secs(60);

/// One reconciliation pass over the cash plane. The fields are raw 18-dp USDT base units
/// and the parked-row count, so a caller (a test, the run loop) can assert on the outcome
/// without scraping logs.
#[derive(Debug, Default, Clone, Copy)]
pub struct ReconReport {
	pub custody: u128,
	pub claims: u128,
	pub clearing_reserved: u128,
	pub clearing_expected: u128,
	pub parked_rows: usize,
	pub uncompensated_parked: usize,
}

impl ReconReport {
	/// Whether every checked invariant held (no operator action needed).
	pub fn clean(&self) -> bool {
		self.custody == self.claims && self.clearing_reserved == self.clearing_expected && self.parked_rows == 0
	}
}

/// The reconciliation job: scan on an interval until the process exits.
pub struct Reconciliation {
	pool: PgPool,
	ledger: Arc<dyn Ledger>,
}

impl Reconciliation {
	pub fn new(pool: PgPool, ledger: Arc<dyn Ledger>) -> Self {
		Self { pool, ledger }
	}

	pub async fn run(self, shutdown: CancellationToken) {
		info!("reconciliation: starting PG-vs-TB scan every {SCAN_INTERVAL:?}");
		loop {
			if let Err(err) = self.scan().await {
				warn!("reconciliation: scan failed (will retry): {err}");
			}
			tokio::select! {
				() = shutdown.cancelled() => return,
				() = tokio::time::sleep(SCAN_INTERVAL) => {},
			}
		}
	}

	/// One scan pass. Public so an integration test can drive it deterministically and
	/// assert a parked event is surfaced. Returns the report; alerts are a side effect.
	pub async fn scan(&self) -> Result<ReconReport, sqlx::Error> {
		let mut report = ReconReport::default();

		// (1) Global cash invariant — sum(custody) == sum(claims), straight from TB.
		match self.ledger.cash_invariant().await {
			Ok(inv) => {
				report.custody = inv.custody;
				report.claims = inv.claims;
				if !inv.balanced() {
					error!(custody = %inv.custody, claims = %inv.claims, "reconciliation: CASH INVARIANT BROKEN — sum(custody) != sum(claims)");
				}
			}
			// A read failure leaves custody/claims at 0, so `clean()` would read the cash leg
			// as a trivially-satisfied `0 == 0`. Alert (not warn): a persistent failure — e.g.
			// an oversized `lookup_accounts` at scale — silently disables the conservation
			// check, and must not be mistaken for a transient TB blip that self-heals.
			Err(err) => error!("reconciliation: CASH INVARIANT UNCHECKED — cash-invariant read failed, conservation not verified this pass: {err}"),
		}

		// (2) Clearing reservation vs the gross of in-flight withdrawals. `queued`/
		// `processing` are the in-flight states; their gross should equal what is reserved
		// (pending + posted-but-not-yet-disbursed) on the clearing claim.
		let expected: Option<String> = sqlx::query_scalar("SELECT COALESCE(SUM(amount::numeric), 0)::text FROM withdrawals WHERE state IN ('queued', 'processing')")
			.fetch_one(&self.pool)
			.await?;
		report.clearing_expected = expected.and_then(|s| s.parse().ok()).unwrap_or(0);
		match self.ledger.balance(&LedgerAccountKey::WithdrawalClearing).await {
			Ok(bal) => {
				report.clearing_reserved = bal.posted.saturating_add(bal.pending);
				if report.clearing_reserved != report.clearing_expected {
					error!(
						reserved = %report.clearing_reserved,
						expected = %report.clearing_expected,
						"reconciliation: CLEARING MISMATCH — reserved on the ledger != gross of in-flight withdrawals (a parked reserve, or a stranded reservation)"
					);
				}
			}
			Err(err) => warn!("reconciliation: clearing balance read failed: {err}"),
		}

		// (3) Parked-row scan — every event the relay moved to the parked terminal state.
		let parked = outbox::parked_rows(&self.pool).await?;
		report.parked_rows = parked.len();
		report.uncompensated_parked = parked.iter().filter(|r| !r.compensated).count();
		for row in &parked {
			error!(
				seq = row.seq,
				event_id = %row.event_id,
				aggregate = %row.aggregate,
				aggregate_id = %row.aggregate_id,
				kind = %row.kind,
				compensated = row.compensated,
				"reconciliation: PARKED outbox event needs intervention: {}",
				row.last_error.as_deref().unwrap_or("(no last_error)")
			);
		}

		if report.clean() {
			info!(custody = %report.custody, "reconciliation: clean — invariants hold, no parked rows");
		}
		Ok(report)
	}
}
