//! On-chain Tron withdrawal confirmation watcher — auto-settles broadcast TRC20 withdrawals.
//!
//! The Tron analogue of [`withdrawal_watcher`](super::withdrawal_watcher): the positive on-chain
//! signal that drives the settle the [`reaper`](super::reaper) can only alert on. It polls each
//! `processing` Tron withdrawal's stored txid and, once it is mined successfully and **solidified**
//! (irreversible — Tron's finality, scoped against the solidified head rather than a confirmation
//! depth), calls [`settle_withdrawal`](crate::application::withdrawals::settle_withdrawal).
//!
//! Split by safety exactly like the BEP20 watcher:
//!   - mined + success + solidified -> AUTO-SETTLE.
//!   - mined + reverted (`receipt.result != SUCCESS`) -> ALERT ONLY (a money decision for an
//!     operator; never auto-failed).
//!   - not yet mined / not yet solidified -> wait.
//!
//! The query is scoped to `network = 'trc20'` so it only ever reads Tron txids (a BEP20/TON row in
//! the shared `withdrawal_broadcasts` would be meaningless to TronGrid). Read-mostly; never touches
//! TigerBeetle.

use std::{sync::Arc, time::Duration};

use domain::{money::TxRef, withdrawals::WithdrawalId};
use sqlx::PgPool;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{application::withdrawals::settle_withdrawal, config::TronConfig, infrastructure::tron_rpc::TronRpc, ports::WithdrawalRepository};

pub struct TronWithdrawalWatcher {
	pool: PgPool,
	rpc: TronRpc,
	withdrawals: Arc<dyn WithdrawalRepository>,
	relay: Arc<Notify>,
	poll: Duration,
}

impl TronWithdrawalWatcher {
	pub fn new(pool: PgPool, withdrawals: Arc<dyn WithdrawalRepository>, relay: Arc<Notify>, config: &TronConfig) -> Self {
		Self {
			pool,
			rpc: TronRpc::new(config.rpc_url.clone(), config.api_key.clone(), config.expiration_secs),
			withdrawals,
			relay,
			poll: Duration::from_secs(config.poll_secs),
		}
	}

	pub async fn run(self, shutdown: CancellationToken) {
		info!("tron withdrawal watcher: confirming broadcast TRC20 withdrawals");
		loop {
			if let Err(err) = self.scan_once().await {
				warn!("tron withdrawal watcher: scan cycle failed, retrying next poll: {err}");
			}
			tokio::select! {
				() = shutdown.cancelled() => {
					info!("tron withdrawal watcher: shutdown requested — stopping");
					return;
				}
				() = tokio::time::sleep(self.poll) => {}
			}
		}
	}

	async fn scan_once(&self) -> Result<(), WatcherError> {
		let pending = self.pending_broadcasts().await?;
		if pending.is_empty() {
			return Ok(()); // nothing in flight — skip the solidified-head read entirely.
		}
		let solid_head = self.rpc.solid_block_number().await.map_err(|e| WatcherError::Rpc(e.to_string()))?;
		for (withdrawal_id, txid) in pending {
			self.check_one(withdrawal_id, &txid, solid_head).await?;
		}
		Ok(())
	}

	/// The `processing` Tron withdrawals we have broadcast. Scoped to `network = 'trc20'` so a
	/// BEP20/TON broadcast row is never read as a Tron txid.
	async fn pending_broadcasts(&self) -> Result<Vec<(Uuid, String)>, WatcherError> {
		sqlx::query_as::<_, (Uuid, String)>(
			"SELECT b.withdrawal_id, b.tx_hash FROM withdrawal_broadcasts b \
			 JOIN withdrawals w ON w.id = b.withdrawal_id WHERE w.state = 'processing' AND b.network = 'trc20'",
		)
		.fetch_all(&self.pool)
		.await
		.map_err(|e| WatcherError::Db(e.to_string()))
	}

	async fn check_one(&self, withdrawal_id: Uuid, txid: &str, solid_head: u64) -> Result<(), WatcherError> {
		let Some(receipt) = self.rpc.transaction_info(txid).await.map_err(|e| WatcherError::Rpc(e.to_string()))? else {
			return Ok(()); // not yet mined — re-check next poll.
		};
		if !receipt.success {
			error!(%withdrawal_id, %txid, "tron withdrawal watcher: broadcast transaction REVERTED on-chain — needs operator review (not auto-failed)");
			return Ok(());
		}
		if receipt.block_number > solid_head {
			return Ok(()); // mined, but not yet solidified (irreversible).
		}
		let tx_ref = TxRef::parse(txid).map_err(|e| WatcherError::Decode(e.to_string()))?;
		match settle_withdrawal(self.withdrawals.as_ref(), &self.relay, WithdrawalId::from_raw(withdrawal_id), tx_ref).await {
			Ok(_) => info!(%withdrawal_id, %txid, "tron withdrawal watcher: confirmed on-chain — settled"),
			Err(err) => warn!(%withdrawal_id, "tron withdrawal watcher: could not settle confirmed withdrawal (will retry next poll): {err}"),
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
