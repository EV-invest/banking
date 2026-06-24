//! Saga reaper — recovery for abandoned/stuck sagas (BANK-FAULT-04).
//!
//! TB pendings are created with `timeout = 0` ("the saga owns the lifecycle, not TB's
//! clock"), so an in-flight reservation never auto-voids. That is correct, but it means an
//! abandoned saga — one whose external completion signal is lost or never arrives — wedges
//! the user's value forever unless *something* owns the timeout. This periodic job is that
//! owner. It is deliberately split by safety, per the cardinal withdrawal rule (never void
//! once the broadcast may have reached the chain):
//!
//!   - **`processing` withdrawals past the max age → ALERT ONLY.** The broadcast may have
//!     landed; auto-voiding would double-pay. Recovery needs a positive not-broadcast /
//!     dropped chain signal (an operator/watcher action), so the reaper only raises a
//!     Sentry-shipped `error!` for a human.
//!   - **`queued` redemptions past the max age → AUTO-FAIL.** A redemption is internal
//!     (claim→claim, nothing leaves the chain), so voiding the pending burn and returning
//!     the units is always safe.
//!   - **`queued` withdrawals past the max age → AUTO-CANCEL.** A queued withdrawal was
//!     never broadcast, so voiding the clearing reservation (full refund) is always safe;
//!     this is the backstop for a treasury worker that never dispatches.
//!
//! The auto-fail/cancel paths go through the same row-locked repository commands a user
//! would (idempotent, drain the void event), so the relay reverses the money exactly once.

use std::{sync::Arc, time::Duration};

use domain::{redemptions::RedemptionId, withdrawals::WithdrawalId};
use sqlx::PgPool;
use tokio::sync::Notify;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::ports::{RedemptionRepository, WithdrawalRepository};

/// How often the reaper sweeps for stuck sagas.
const SWEEP_INTERVAL: Duration = Duration::from_secs(60);

/// A saga older than this (since its last transition) is abandoned. Conservative — well
/// past any legitimate operator/watcher settle window — because the safe paths void real
/// reservations and the `processing` path pages a human. 24h for v1, matching the NAV
/// staleness horizon.
const MAX_SAGA_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// One reaper sweep's outcome — counts so the run loop and a test can assert without
/// scraping logs.
#[derive(Debug, Default, Clone, Copy)]
pub struct ReaperReport {
	/// `processing` withdrawals past the max age — alerted, never auto-failed.
	pub stuck_processing_withdrawals: usize,
	/// `queued` withdrawals auto-cancelled (refunded).
	pub reaped_queued_withdrawals: usize,
	/// `queued` redemptions auto-failed (units returned).
	pub reaped_queued_redemptions: usize,
}

/// The reaper job: sweep on an interval until the process exits.
pub struct Reaper {
	pool: PgPool,
	withdrawals: Arc<dyn WithdrawalRepository>,
	redemptions: Arc<dyn RedemptionRepository>,
	notify: Arc<Notify>,
	max_age: Duration,
}

impl Reaper {
	pub fn new(pool: PgPool, withdrawals: Arc<dyn WithdrawalRepository>, redemptions: Arc<dyn RedemptionRepository>, notify: Arc<Notify>) -> Self {
		Self {
			pool,
			withdrawals,
			redemptions,
			notify,
			max_age: MAX_SAGA_AGE,
		}
	}

	/// Test seam: a shorter abandonment window so an integration test need not wait 24h.
	pub fn with_max_age(mut self, max_age: Duration) -> Self {
		self.max_age = max_age;
		self
	}

	pub async fn run(self) {
		info!("reaper: sweeping abandoned sagas every {SWEEP_INTERVAL:?} (max age {:?})", self.max_age);
		loop {
			if let Err(err) = self.sweep().await {
				warn!("reaper: sweep failed (will retry): {err}");
			}
			tokio::time::sleep(SWEEP_INTERVAL).await;
		}
	}

	/// One sweep. Public so an integration test can drive it deterministically. Alerts on
	/// stuck `processing` withdrawals and auto-resolves the safe `queued` sagas.
	pub async fn sweep(&self) -> Result<ReaperReport, sqlx::Error> {
		let mut report = ReaperReport::default();
		let cutoff_secs = self.max_age.as_secs() as i64;

		// (1) `processing` withdrawals past the cutoff — ALERT ONLY (may be broadcast).
		let stuck: Vec<(Uuid, String)> = sqlx::query_as("SELECT id, user_id::text FROM withdrawals WHERE state = 'processing' AND updated_at < now() - make_interval(secs => $1)")
			.bind(cutoff_secs)
			.fetch_all(&self.pool)
			.await?;
		report.stuck_processing_withdrawals = stuck.len();
		for (id, user) in &stuck {
			error!(
				withdrawal_id = %id,
				user_id = %user,
				"reaper: STUCK processing withdrawal past max age — needs a confirmed not-broadcast signal before fail/void (never auto-voided)"
			);
		}

		// (2) `queued` withdrawals past the cutoff — AUTO-CANCEL (never broadcast → safe).
		let stale_withdrawals: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM withdrawals WHERE state = 'queued' AND updated_at < now() - make_interval(secs => $1)")
			.bind(cutoff_secs)
			.fetch_all(&self.pool)
			.await?;
		for id in stale_withdrawals {
			match self.withdrawals.cancel(WithdrawalId::from_raw(id)).await {
				Ok(_) => {
					report.reaped_queued_withdrawals += 1;
					warn!(withdrawal_id = %id, "reaper: auto-cancelled abandoned queued withdrawal (refunded)");
				}
				Err(err) => warn!(withdrawal_id = %id, "reaper: could not auto-cancel queued withdrawal: {err}"),
			}
		}

		// (3) `queued` redemptions past the cutoff — AUTO-FAIL (internal claim→claim → safe).
		let stale_redemptions: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM redemptions WHERE state = 'queued' AND updated_at < now() - make_interval(secs => $1)")
			.bind(cutoff_secs)
			.fetch_all(&self.pool)
			.await?;
		for id in stale_redemptions {
			match self.redemptions.fail(RedemptionId::from_raw(id)).await {
				Ok(_) => {
					report.reaped_queued_redemptions += 1;
					warn!(redemption_id = %id, "reaper: auto-failed abandoned queued redemption (units returned)");
				}
				Err(err) => warn!(redemption_id = %id, "reaper: could not auto-fail queued redemption: {err}"),
			}
		}

		if report.reaped_queued_withdrawals > 0 || report.reaped_queued_redemptions > 0 {
			self.notify.notify_one();
		}
		Ok(report)
	}
}
