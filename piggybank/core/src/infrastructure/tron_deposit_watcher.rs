//! On-chain Tron deposit watcher — credits user balances from confirmed TRC20 USDT transfers.
//!
//! The Tron analogue of [`deposit_watcher`](super::deposit_watcher). Two structural differences
//! from the EVM path:
//!   - There is no `eth_getLogs` OR-filter. The indexed `/v1/accounts/{addr}/transactions/trc20`
//!     surface is **per address**, so the watcher iterates the watched set rather than one ranged
//!     log query; the cursor is a `block_timestamp` (ms) high-watermark, not a block height.
//!   - Confirmations are categorical, not depth-counted: `only_confirmed=true` returns only
//!     solidified (irreversible) transfers, so there is no `latest - N` arithmetic.
//!
//! Everything else mirrors the BEP20 watcher: credit each transfer via
//! [`record_deposit`](crate::application::balance::record_deposit) — idempotent by the on-chain
//! `transaction_id`, so a re-scan never double-credits — and the relay posts the money (the
//! watcher never touches TigerBeetle). USDT on Tron is **6-dp**, so credits scale through
//! [`Usdt::from_onchain`] (a 10^12 difference from BEP20's 18-dp).

use std::{collections::HashMap, sync::Arc, time::Duration};

use domain::{
	balance::Party,
	money::{Network, TxRef, Usdt},
	users::UserId,
};
use sqlx::PgPool;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::{
	application::balance::record_deposit,
	config::TronConfig,
	infrastructure::{
		deposits::PgDeposits,
		tron_rpc::{Trc20Transfer, TronRpc},
	},
};

pub struct TronDepositWatcher {
	pool: PgPool,
	deposits: PgDeposits,
	relay: Arc<Notify>,
	rpc: TronRpc,
	usdt_contract: String,
	start_timestamp: Option<i64>,
	max_transfers: u32,
	poll: Duration,
}

impl TronDepositWatcher {
	pub fn new(pool: PgPool, relay: Arc<Notify>, config: &TronConfig) -> Self {
		Self {
			deposits: PgDeposits::new(pool.clone()),
			pool,
			relay,
			rpc: TronRpc::new(config.rpc_url.clone(), config.api_key.clone(), config.expiration_secs),
			usdt_contract: config.usdt_contract.clone(),
			start_timestamp: config.start_timestamp,
			max_transfers: config.max_transfers_per_scan,
			poll: Duration::from_secs(config.poll_secs),
		}
	}

	/// Poll until `shutdown` is cancelled. A failed cycle is logged and retried next poll from the
	/// unchanged cursor — at-least-once, and crediting is idempotent, so nothing is lost or doubled.
	pub async fn run(self, shutdown: CancellationToken) {
		info!(contract = %self.usdt_contract, "tron deposit watcher: watching TRC20 USDT deposits");
		loop {
			if let Err(err) = self.scan_once().await {
				warn!("tron deposit watcher: scan cycle failed, retrying next poll: {err}");
			}
			tokio::select! {
				() = shutdown.cancelled() => {
					info!("tron deposit watcher: shutdown requested — stopping");
					return;
				}
				() = tokio::time::sleep(self.poll) => {}
			}
		}
	}

	async fn scan_once(&self) -> Result<(), WatcherError> {
		let network = Network::Trc20;
		let watched = self.watched_addresses(network).await?;
		if watched.is_empty() {
			return Ok(()); // nothing fundable yet — don't even establish a cursor.
		}
		let cursor = self.cursor(network).await?;
		let mut high_watermark = cursor;
		for (address, user) in &watched {
			// Drain this address to the head, page by page: a single capped page could leave
			// older transfers unfetched while ANOTHER address's newer transfer advances the
			// global watermark past them — skipped forever. Paging until a short raw page
			// makes the post-loop watermark advance safe for every address.
			let mut address_from = cursor;
			loop {
				let page = self
					.rpc
					.incoming_trc20(address, &self.usdt_contract, address_from, self.max_transfers)
					.await
					.map_err(|e| WatcherError::Rpc(e.to_string()))?;
				for transfer in &page.transfers {
					self.credit(*user, network, transfer).await?;
					high_watermark = high_watermark.max(transfer.block_timestamp);
				}
				if page.raw_len < self.max_transfers as usize {
					break;
				}
				if page.max_timestamp <= address_from {
					// A full page that cannot advance the window (one timestamp). Defer: the
					// unmoved cursor re-scans next cycle; crediting is idempotent by tx id.
					warn!(address, "tron deposit watcher: full page without time progress — deferring scan");
					return Ok(());
				}
				// Resume AT the newest raw timestamp (inclusive): boundary rows are refetched
				// and deduped, filtered rows still advance the window.
				address_from = page.max_timestamp;
			}
		}
		// Advance only after the window's deposits are recorded. A crash before this re-scans from
		// the unchanged cursor; `record_deposit` is idempotent by transaction_id.
		if high_watermark > cursor {
			self.set_cursor(network, high_watermark).await?;
		}
		Ok(())
	}

	async fn credit(&self, user: UserId, network: Network, transfer: &Trc20Transfer) -> Result<(), WatcherError> {
		let amount = Usdt::from_onchain(network, transfer.value).map_err(|e| WatcherError::Decode(e.to_string()))?;
		if amount.is_zero() {
			return Ok(()); // a legal but meaningless zero-value transfer — not a deposit.
		}
		let tx_ref = TxRef::parse(&transfer.transaction_id).map_err(|e| WatcherError::Decode(e.to_string()))?;
		let newly = record_deposit(&self.deposits, &self.relay, tx_ref, Party::User(user), network, amount)
			.await
			.map_err(|e| WatcherError::Credit(e.to_string()))?;
		if newly {
			info!(user = %user, tx = %transfer.transaction_id, "tron deposit watcher: credited on-chain USDT deposit");
		}
		Ok(())
	}

	async fn watched_addresses(&self, network: Network) -> Result<HashMap<String, UserId>, WatcherError> {
		let rows: Vec<(uuid::Uuid, String)> = sqlx::query_as("SELECT user_id, address FROM user_deposit_addresses WHERE network = $1 AND address_kind = 'derived'")
			.bind(network.as_str())
			.fetch_all(&self.pool)
			.await
			.map_err(repo)?;
		Ok(rows.into_iter().map(|(uid, address)| (address, UserId::from_raw(uid))).collect())
	}

	/// The deposit-scan high-watermark (a `block_timestamp` in ms). On first run, initialize to the
	/// configured start timestamp, else the current head (watch from now), so pre-existing history
	/// is ignored. The `deposit_scan_cursor.last_scanned_block` bigint holds the opaque ms value.
	async fn cursor(&self, network: Network) -> Result<i64, WatcherError> {
		if let Some(existing) = sqlx::query_scalar::<_, i64>("SELECT last_scanned_block FROM deposit_scan_cursor WHERE network = $1")
			.bind(network.as_str())
			.fetch_optional(&self.pool)
			.await
			.map_err(repo)?
		{
			return Ok(existing);
		}
		let init = match self.start_timestamp {
			Some(ts) => ts,
			None => self.rpc.ref_block_params().await.map_err(|e| WatcherError::Rpc(e.to_string()))?.timestamp,
		};
		sqlx::query("INSERT INTO deposit_scan_cursor (network, last_scanned_block) VALUES ($1, $2) ON CONFLICT (network) DO NOTHING")
			.bind(network.as_str())
			.bind(init)
			.execute(&self.pool)
			.await
			.map_err(repo)?;
		Ok(init)
	}

	async fn set_cursor(&self, network: Network, timestamp: i64) -> Result<(), WatcherError> {
		sqlx::query("UPDATE deposit_scan_cursor SET last_scanned_block = $2, updated_at = now() WHERE network = $1")
			.bind(network.as_str())
			.bind(timestamp)
			.execute(&self.pool)
			.await
			.map_err(repo)?;
		Ok(())
	}
}

#[derive(Debug, thiserror::Error)]
enum WatcherError {
	#[error("rpc: {0}")]
	Rpc(String),
	#[error("decode: {0}")]
	Decode(String),
	#[error("credit: {0}")]
	Credit(String),
	#[error("db: {0}")]
	Db(String),
}

fn repo(err: sqlx::Error) -> WatcherError {
	WatcherError::Db(err.to_string())
}
