//! On-chain TON deposit watcher — credits user balances from incoming USDT (jetton)
//! transfers.
//!
//! The TON sibling of [`deposit_watcher`](super::deposit_watcher). USDT on TON is a
//! TEP-74 **jetton**, so a deposit lands in the user's derived wallet's *jetton wallet*
//! contract; we attribute it via toncenter's server-side `owner_address` filter on the
//! decoded `/jetton/transfers` feed (no client-side address decoding, no `eth_getLogs`).
//! Each transfer is recorded via [`record_deposit`](crate::application::balance::record_deposit)
//! — idempotent by `transaction_hash:user`, so a re-scan never double-credits — and
//! the relay then posts `Dr wallet:ton / Cr user-claim`; the watcher never touches
//! TigerBeetle, so money is still written last, in the relay.
//!
//! **Finality.** TON is fast and categorical: toncenter only surfaces transactions from
//! committed masterchain blocks, so anything `/jetton/transfers` returns is already final —
//! there is no N-confirmations counter (unlike BEP20).
//!
//! **Cursor (deliberate divergence from per-account `lt`).** A single network-scoped row in
//! `deposit_scan_cursor` holds a **unix-time** high-watermark (`transaction_now`), not a
//! logical time. A logical time (`lt`) is monotonic only *per account*, so one network-wide
//! `lt` cursor over many owners would skip a lagging owner's deposit outright. A wall-clock
//! watermark is globally comparable, so the only way it drops a deposit is if the indexer
//! surfaces a final transaction MORE than `LOOKBACK_SECS` after its `transaction_now` while a
//! busier owner has already pushed the watermark past it — i.e. the safety margin is exactly
//! `LOOKBACK_SECS` of effective indexer lag, not infinite. It is set generously for that
//! reason. Each cycle re-scans that window below the watermark; the overlap is idempotent
//! (`record_deposit` dedupes by `transaction_hash`).

use std::{sync::Arc, time::Duration};

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
	config::TonConfig,
	infrastructure::{
		deposits::PgDeposits,
		ton_rpc::{JettonDeposit, TonRpc},
	},
};

/// Per-owner page size for `/jetton/transfers`.
const PAGE_LIMIT: u32 = 128;

/// Seconds re-scanned below the cursor each cycle, to absorb indexer lag (the indexer can
/// surface a final transaction after its `transaction_now`). This IS the safety margin against
/// dropping a lagging owner's deposit (see the module docstring), so it is set well above
/// toncenter's observed lag rather than at the bare minimum. The overlap is idempotent —
/// `record_deposit` dedupes by `transaction_hash`.
const LOOKBACK_SECS: u64 = 900;

pub struct TonDepositWatcher {
	pool: PgPool,
	deposits: PgDeposits,
	relay: Arc<Notify>,
	rpc: TonRpc,
	config: TonConfig,
}

impl TonDepositWatcher {
	pub fn new(pool: PgPool, relay: Arc<Notify>, config: TonConfig) -> Self {
		let rpc = TonRpc::new(config.api_url.clone(), config.api_key.clone());
		let deposits = PgDeposits::new(pool.clone());
		Self { pool, deposits, relay, rpc, config }
	}

	/// Poll until `shutdown` is cancelled. A failed cycle is logged and retried next poll
	/// from the unchanged cursor — at-least-once, and crediting is idempotent.
	pub async fn run(self, shutdown: CancellationToken) {
		info!(master = %self.config.usdt_master, testnet = self.config.is_testnet, "ton deposit watcher: watching jetton USDT deposits");
		loop {
			if let Err(err) = self.scan_once().await {
				warn!("ton deposit watcher: scan cycle failed, retrying next poll: {err}");
			}
			tokio::select! {
				() = shutdown.cancelled() => {
					info!("ton deposit watcher: shutdown requested — stopping");
					return;
				}
				() = tokio::time::sleep(Duration::from_secs(self.config.poll_secs)) => {}
			}
		}
	}

	async fn scan_once(&self) -> Result<(), WatcherError> {
		let network = Network::Ton;
		let cursor = self.cursor(network).await?;
		let watched = self.watched_addresses(network).await?;
		if watched.is_empty() {
			// Nothing fundable yet — fast-forward to now so we don't re-scan an empty window.
			self.set_cursor(network, now_unix()).await?;
			return Ok(());
		}

		let from = cursor.saturating_sub(LOOKBACK_SECS);
		let mut high = cursor;
		for (owner, user) in &watched {
			// Drain this owner to the head, page by page: a single capped page could leave
			// older transfers unfetched while ANOTHER owner's newer transfer advances the
			// global watermark past them — skipped forever. Paging until a short raw page
			// makes the post-loop watermark advance safe for every owner.
			let mut owner_from = from;
			loop {
				let page = match self.rpc.incoming_jetton_transfers(owner, &self.config.usdt_master, owner_from, PAGE_LIMIT).await {
					Ok(page) => page,
					Err(err) => {
						// One owner's fetch failing MUST NOT abort the whole cycle: a `?` here froze the
						// entire TON rail on a single unparseable stored address (indexer 422) — the
						// cursor never advanced and NO user's deposits were seen. Skip just this owner;
						// a transiently-failed valid owner is re-scanned within the LOOKBACK_SECS window
						// next cycle, and a permanently-bad address is never a real deposit target.
						warn!(owner, "ton deposit watcher: owner scan failed, skipping this owner this cycle: {err}");
						break;
					}
				};
				for transfer in &page.transfers {
					self.credit(*user, network, transfer).await?;
					high = high.max(transfer.now);
				}
				if page.raw_len < PAGE_LIMIT as usize {
					break;
				}
				if page.max_now <= owner_from {
					// A full page whose entries all share the start second, so time-paging can't
					// advance within THIS owner (≥128 credits to one address in one second — a
					// pathological case a time-cursor indexer genuinely can't page past). The 128
					// fetched here were credited; break to the NEXT owner rather than `return`ing
					// out of the whole cycle, which would starve every later owner forever (the
					// cursor never advances, so each cycle re-hits this same stuck page first). The
					// `LOOKBACK_SECS` window re-scans this owner next cycle; crediting is idempotent.
					warn!(owner, "ton deposit watcher: full page without time progress — skipping this owner for the cycle");
					break;
				}
				// Resume AT the newest raw time (inclusive): boundary entries are refetched
				// and deduped, filtered rows still advance the window.
				owner_from = page.max_now;
			}
		}
		// Advance to the newest transaction time seen (never backwards). The next cycle
		// re-scans `LOOKBACK_SECS` below this; the overlap is deduped by `record_deposit`.
		if high > cursor {
			self.set_cursor(network, high).await?;
		}
		Ok(())
	}

	async fn credit(&self, user: UserId, network: Network, transfer: &JettonDeposit) -> Result<(), WatcherError> {
		let amount = Usdt::from_onchain(network, transfer.amount).map_err(|e| WatcherError::Decode(e.to_string()))?;
		if amount.is_zero() {
			return Ok(());
		}
		// Disambiguate per recipient like the BEP20/TRC20 watchers: `deposits.tx_ref` is a global
		// key, so compose the on-chain transaction hash with the credited user. In practice each
		// incoming jetton transfer is its own transaction on the recipient's jetton wallet (so the
		// hash is already unique), but the user id makes two transfers under one hash — an indexer
		// quirk — impossible to collapse across users. The user id (a 36-char uuid) keeps the key
		// well under `TxRef`'s length cap regardless of the indexer's hash encoding, and is stable
		// across re-scans (the address→user map is fixed), so idempotency holds.
		let tx_ref = TxRef::parse(&format!("{}:{user}", transfer.tx_hash)).map_err(|e| WatcherError::Decode(e.to_string()))?;
		let newly = record_deposit(&self.deposits, &self.relay, tx_ref, Party::User(user), network, amount)
			.await
			.map_err(|e| WatcherError::Credit(e.to_string()))?;
		if newly {
			info!(user = %user, tx = %transfer.tx_hash, "ton deposit watcher: credited on-chain jetton USDT deposit");
		}
		Ok(())
	}

	/// The deposit cursor (a unix-time watermark). On first run, initialize to the configured
	/// start (`TON_DEPOSIT_START_UTIME`, unix seconds) or the current time (watch from now),
	/// ignoring pre-existing on-chain history.
	async fn cursor(&self, network: Network) -> Result<u64, WatcherError> {
		let existing: Option<i64> = sqlx::query_scalar("SELECT last_scanned_block FROM deposit_scan_cursor WHERE network = $1")
			.bind(network.as_str())
			.fetch_optional(&self.pool)
			.await
			.map_err(repo)?;
		if let Some(cursor) = existing {
			return Ok(cursor.max(0) as u64);
		}
		let init = self.config.start_cursor.unwrap_or_else(now_unix);
		sqlx::query("INSERT INTO deposit_scan_cursor (network, last_scanned_block) VALUES ($1, $2) ON CONFLICT (network) DO NOTHING")
			.bind(network.as_str())
			.bind(init as i64)
			.execute(&self.pool)
			.await
			.map_err(repo)?;
		Ok(init)
	}

	async fn set_cursor(&self, network: Network, cursor: u64) -> Result<(), WatcherError> {
		sqlx::query("UPDATE deposit_scan_cursor SET last_scanned_block = $2, updated_at = now() WHERE network = $1")
			.bind(network.as_str())
			.bind(cursor as i64)
			.execute(&self.pool)
			.await
			.map_err(repo)?;
		Ok(())
	}

	/// The watched (owner address → user) map: only `derived` (fundable) TON addresses. The
	/// stored address is the raw `0:hex` owner wallet, passed straight to toncenter's
	/// `owner_address` filter.
	async fn watched_addresses(&self, network: Network) -> Result<Vec<(String, UserId)>, WatcherError> {
		let rows: Vec<(uuid::Uuid, String)> = sqlx::query_as("SELECT user_id, address FROM user_deposit_addresses WHERE network = $1 AND address_kind = 'derived'")
			.bind(network.as_str())
			.fetch_all(&self.pool)
			.await
			.map_err(repo)?;
		Ok(rows.into_iter().map(|(uid, address)| (address, UserId::from_raw(uid))).collect())
	}
}

/// Current unix time in seconds.
fn now_unix() -> u64 {
	std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
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
