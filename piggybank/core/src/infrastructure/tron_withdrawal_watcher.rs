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
//!   - not yet mined -> re-drive via custody (see below), then wait.
//!   - not yet solidified -> wait.
//!
//! **Re-driving a stuck send (the TRON-specific liveness gate).** A TRON transaction EXPIRES
//! (~60s) and the rail has no nonce, so once a broadcast send expires in the mempool without
//! landing it can NEVER mine — and the relay dispatches `broadcast` only once, so nothing would
//! ever re-sign it. This watcher therefore holds the [`TronCustody`] adapter and, for a send with
//! no receipt yet, asks it to re-drive: re-broadcast the stored bytes while still live, or re-sign
//! at a fresh ref-block once the send is provably dead. Without this a stuck withdrawal wedges in
//! `processing` forever (the BEP20 watcher can simply wait because EVM txs never expire; the TON
//! watcher does the analogous re-drive for its expiring sends).
//!
//! The query is scoped to `network = 'trc20'` so it only ever reads Tron txids (a BEP20/TON row in
//! the shared `withdrawal_broadcasts` would be meaningless to TronGrid). Read-mostly; never touches
//! TigerBeetle.

use std::{sync::Arc, time::Duration};

use domain::{
	money::{Network, TxRef, Usdt, WalletAddress},
	withdrawals::WithdrawalId,
};
use sqlx::PgPool;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{
	application::withdrawals::settle_withdrawal,
	config::TronConfig,
	infrastructure::{tron_custody::TronCustody, tron_rpc::TronRpc},
	ports::{
		WithdrawalRepository,
		custody::{BroadcastRequest, CustodyError},
	},
};

pub struct TronWithdrawalWatcher {
	pool: PgPool,
	rpc: TronRpc,
	custody: Arc<TronCustody>,
	withdrawals: Arc<dyn WithdrawalRepository>,
	relay: Arc<Notify>,
	poll: Duration,
}

impl TronWithdrawalWatcher {
	pub fn new(pool: PgPool, custody: Arc<TronCustody>, withdrawals: Arc<dyn WithdrawalRepository>, relay: Arc<Notify>, config: &TronConfig) -> Self {
		Self {
			pool,
			rpc: TronRpc::new(config.rpc_url.clone(), config.api_key.clone(), config.expiration_secs),
			custody,
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
		for p in &pending {
			self.check_one(p, solid_head).await?;
		}
		Ok(())
	}

	/// The `processing` Tron withdrawals we have broadcast, with the stored txid and the
	/// destination/amount needed to re-drive a stuck send. Scoped to `network = 'trc20'` so a
	/// BEP20/TON broadcast row is never read as a Tron txid.
	async fn pending_broadcasts(&self) -> Result<Vec<TronPending>, WatcherError> {
		let rows = sqlx::query_as::<_, PendingRow>(
			"SELECT b.withdrawal_id, b.tx_hash, w.address, w.amount, w.fee FROM withdrawal_broadcasts b \
			 JOIN withdrawals w ON w.id = b.withdrawal_id WHERE w.state = 'processing' AND b.network = 'trc20'",
		)
		.fetch_all(&self.pool)
		.await
		.map_err(|e| WatcherError::Db(e.to_string()))?;
		let mut pending = Vec::with_capacity(rows.len());
		for row in rows {
			let withdrawal_id = row.withdrawal_id;
			match TronPending::build(row) {
				Ok(p) => pending.push(p),
				Err(err) => warn!(%withdrawal_id, "tron withdrawal watcher: skipping a malformed pending broadcast: {err}"),
			}
		}
		Ok(pending)
	}

	async fn check_one(&self, pending: &TronPending, solid_head: u64) -> Result<(), WatcherError> {
		let Some(receipt) = self.rpc.transaction_info(&pending.txid).await.map_err(|e| WatcherError::Rpc(e.to_string()))? else {
			// Not yet mined. A TRON send expires (~60s) and has no nonce, so if it expired
			// unlanded it can NEVER mine — re-drive via custody (re-broadcast while live, re-sign
			// once provably dead) rather than waiting forever.
			self.redrive(pending).await;
			return Ok(());
		};
		if !receipt.success {
			error!(withdrawal_id = %pending.withdrawal_id, txid = %pending.txid, "tron withdrawal watcher: broadcast transaction REVERTED on-chain — needs operator review (not auto-failed)");
			return Ok(());
		}
		if receipt.block_number > solid_head {
			return Ok(()); // mined, but not yet solidified (irreversible).
		}
		let tx_ref = TxRef::parse(&pending.txid).map_err(|e| WatcherError::Decode(e.to_string()))?;
		match settle_withdrawal(self.withdrawals.as_ref(), &self.relay, WithdrawalId::from_raw(pending.withdrawal_id), tx_ref).await {
			Ok(_) => info!(withdrawal_id = %pending.withdrawal_id, txid = %pending.txid, "tron withdrawal watcher: confirmed on-chain — settled"),
			Err(err) => warn!(withdrawal_id = %pending.withdrawal_id, "tron withdrawal watcher: could not settle confirmed withdrawal (will retry next poll): {err}"),
		}
		Ok(())
	}

	/// Ask custody to re-drive a not-yet-mined send. A "past expiration but not provably dead
	/// yet" reply comes back `Unavailable` — a transient wait, retried next poll — while a genuine
	/// rejection (e.g. treasury re-underfunded on a re-sign) is a park needing intervention.
	async fn redrive(&self, pending: &TronPending) {
		match self.custody.resubmit_stuck(&pending.request).await {
			Ok(()) => {} // re-broadcast the stored bytes, or re-signed a provably-dead one (custody logs which).
			Err(CustodyError::Unavailable(msg)) =>
				info!(withdrawal_id = %pending.withdrawal_id, "tron withdrawal watcher: stuck send not yet re-drivable (waiting for solidification): {msg}"),
			Err(CustodyError::Rejected(msg)) => warn!(withdrawal_id = %pending.withdrawal_id, "tron withdrawal watcher: could not recover a stuck send (needs intervention): {msg}"),
		}
	}
}

/// The raw join row backing a [`TronPending`] (`withdrawal_broadcasts` ⋈ `withdrawals`).
#[derive(sqlx::FromRow)]
struct PendingRow {
	withdrawal_id: Uuid,
	tx_hash: String,
	address: String,
	amount: String,
	fee: String,
}

/// A `processing` Tron withdrawal's stored broadcast plus the reconstructed request needed to
/// re-drive it if the send is stuck.
struct TronPending {
	withdrawal_id: Uuid,
	txid: String,
	request: BroadcastRequest,
}

impl TronPending {
	fn build(row: PendingRow) -> Result<Self, String> {
		let gross = Usdt::from_base_units(row.amount.parse::<u128>().map_err(|_| "malformed amount")?);
		let fee = Usdt::from_base_units(row.fee.parse::<u128>().map_err(|_| "malformed fee")?);
		let net = gross.checked_sub(fee).ok_or("fee exceeds amount")?;
		let address = WalletAddress::parse(Network::Trc20, &row.address).map_err(|e| e.to_string())?;
		Ok(Self {
			withdrawal_id: row.withdrawal_id,
			txid: row.tx_hash,
			request: BroadcastRequest {
				withdrawal_id: row.withdrawal_id,
				network: Network::Trc20,
				address,
				amount: net,
			},
		})
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
