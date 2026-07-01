//! On-chain withdrawal confirmation watcher — auto-settles broadcast withdrawals.
//!
//! [`ChainCustody`](super::custody::ChainCustody) broadcasts a withdrawal's transfer
//! (`Dispatched` → `Processing`), but the ledger keeps the gross reserved in clearing
//! until the withdrawal *settles*. This background task is the positive on-chain signal
//! that drives that settle — the gap the [`reaper`](super::reaper) documents (it can only
//! *alert* on a stuck `processing` withdrawal because it has no not-broadcast/confirmed
//! signal). It polls each `processing` withdrawal's stored broadcast receipt, and once it
//! is mined successfully and `confirmations` deep, calls
//! [`settle_withdrawal`](crate::application::withdrawals::settle_withdrawal) — the same
//! row-locked command an operator's `SettleWithdrawal` runs, so the relay posts the
//! clearing pending and moves the net out of the rail's custody exactly once (idempotent:
//! the aggregate no-ops a re-settle, and a settled withdrawal leaves the `processing` set).
//!
//! Split by safety, exactly like the reaper (the cardinal rule cuts both ways):
//!   - **mined + success + N confirmations → AUTO-SETTLE.** Unambiguous: the funds left.
//!   - **mined + reverted (`status 0x0`) → ALERT ONLY.** The transfer moved no funds, but
//!     auto-refunding is a money decision (why did our own transfer revert — token paused?
//!     blacklist?) left to an operator; the reaper's stuck-`processing` alert is the
//!     backstop. Never auto-failed here.
//!   - **not yet mined / not deep enough → wait.** Re-checked next poll.
//!
//! Scope: **BEP20 only** (the one rail [`ChainCustody`](super::custody::ChainCustody)
//! broadcasts). Read-mostly; it never touches TigerBeetle — money is still written last,
//! in the relay.

use std::{sync::Arc, time::Duration};

use domain::{money::TxRef, withdrawals::WithdrawalId};
use sqlx::PgPool;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{application::withdrawals::settle_withdrawal, config::BscConfig, infrastructure::bsc_rpc::BscRpc, ports::WithdrawalRepository};

/// The withdrawal confirmation watcher. Holds its own pool clone (its polling reads stay
/// off the request path), the withdrawal repository + relay `Notify` to drive the settle,
/// and a BSC client for the confirmation reads.
pub struct WithdrawalWatcher {
	pool: PgPool,
	rpc: BscRpc,
	withdrawals: Arc<dyn WithdrawalRepository>,
	relay: Arc<Notify>,
	confirmations: u64,
	poll: Duration,
}

impl WithdrawalWatcher {
	pub fn new(pool: PgPool, withdrawals: Arc<dyn WithdrawalRepository>, relay: Arc<Notify>, config: &BscConfig) -> Self {
		Self {
			pool,
			rpc: BscRpc::new(config.rpc_url.clone()),
			withdrawals,
			relay,
			confirmations: config.confirmations,
			poll: Duration::from_secs(config.poll_secs),
		}
	}

	/// Poll until `shutdown` is cancelled. A failed cycle is logged and retried next poll —
	/// settle is idempotent and a settled withdrawal drops out of the `processing` set, so
	/// nothing is double-settled or lost.
	pub async fn run(self, shutdown: CancellationToken) {
		info!(confirmations = self.confirmations, "withdrawal watcher: confirming broadcast BEP20 withdrawals");
		loop {
			if let Err(err) = self.scan_once().await {
				warn!("withdrawal watcher: scan cycle failed, retrying next poll: {err}");
			}
			tokio::select! {
				() = shutdown.cancelled() => {
					info!("withdrawal watcher: shutdown requested — stopping");
					return;
				}
				() = tokio::time::sleep(self.poll) => {}
			}
		}
	}

	async fn scan_once(&self) -> Result<(), WatcherError> {
		let pending = self.pending_broadcasts().await?;
		if pending.is_empty() {
			return Ok(()); // nothing in flight — skip the head read entirely.
		}
		let latest = self.rpc.block_number().await.map_err(|e| WatcherError::Rpc(e.to_string()))?;
		for (withdrawal_id, tx_hash) in pending {
			self.check_one(withdrawal_id, &tx_hash, latest).await?;
		}
		Ok(())
	}

	/// The `processing` withdrawals we have broadcast. The withdrawal *state* is the source
	/// of truth — a settled one leaves the set as its state moves to `completed`, so a
	/// re-settle is never even attempted.
	async fn pending_broadcasts(&self) -> Result<Vec<(Uuid, String)>, WatcherError> {
		// Scoped to `network = 'bep20'` so a Tron/TON broadcast row in the shared table is never
		// read as an EVM tx hash (its receipt lookup would be meaningless on this rail).
		sqlx::query_as::<_, (Uuid, String)>(
			"SELECT b.withdrawal_id, b.tx_hash FROM withdrawal_broadcasts b \
			 JOIN withdrawals w ON w.id = b.withdrawal_id WHERE w.state = 'processing' AND b.network = 'bep20'",
		)
		.fetch_all(&self.pool)
		.await
		.map_err(|e| WatcherError::Db(e.to_string()))
	}

	async fn check_one(&self, withdrawal_id: Uuid, tx_hash: &str, latest: u64) -> Result<(), WatcherError> {
		let Some(receipt) = self.rpc.transaction_receipt(tx_hash).await.map_err(|e| WatcherError::Rpc(e.to_string()))? else {
			return Ok(()); // not yet mined — re-check next poll.
		};
		if !receipt.success {
			// Reverted: the transfer moved no funds. Refunding is an operator decision (the
			// reaper's stuck-`processing` alert is the backstop), never auto-failed here.
			error!(%withdrawal_id, %tx_hash, "withdrawal watcher: broadcast transaction REVERTED on-chain — needs operator review (not auto-failed)");
			return Ok(());
		}
		if latest < receipt.block_number + self.confirmations {
			return Ok(()); // mined, but not yet `confirmations` deep.
		}
		let tx_ref = TxRef::parse(tx_hash).map_err(|e| WatcherError::Decode(e.to_string()))?;
		match settle_withdrawal(self.withdrawals.as_ref(), &self.relay, WithdrawalId::from_raw(withdrawal_id), tx_ref).await {
			Ok(_) => info!(%withdrawal_id, %tx_hash, "withdrawal watcher: confirmed on-chain — settled"),
			Err(err) => warn!(%withdrawal_id, "withdrawal watcher: could not settle confirmed withdrawal (will retry next poll): {err}"),
		}
		Ok(())
	}
}

#[derive(Debug, thiserror::Error)]
enum WatcherError {
	#[error("rpc: {0}")]
	Rpc(String),
	#[error("db: {0}")]
	Db(String),
	#[error("decode: {0}")]
	Decode(String),
}
