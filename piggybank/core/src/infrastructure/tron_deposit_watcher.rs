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
//! `transaction_id:to` (a single Tron tx can pay several of our addresses, so the recipient
//! disambiguates like BEP20's `txhash:logindex`), so a re-scan never double-credits — and the
//! relay posts the money (the watcher never touches TigerBeetle). USDT on Tron is **6-dp**, so
//! credits scale through [`Usdt::from_onchain`] (a 10^12 difference from BEP20's 18-dp).

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

/// Milliseconds re-scanned below the cursor each cycle, to absorb indexer lag — the analogue
/// of the BEP20 watcher's confirmation depth and the TON watcher's `LOOKBACK_SECS`. Without
/// it, a transfer the indexer solidifies with a `block_timestamp` just below an already-
/// advanced watermark (out-of-order/backfill lag) would be skipped forever. The overlap is
/// harmless: `record_deposit` dedupes by `tx_ref`.
const LOOKBACK_MS: i64 = 120_000;

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
			//
			// Start a `LOOKBACK_MS` window below the cursor so a transfer the indexer surfaced
			// just under the watermark (indexing lag) is still picked up — the credit is
			// idempotent by `tx_ref`, so refetching the overlap costs nothing.
			let mut address_from = cursor.saturating_sub(LOOKBACK_MS).max(0);
			loop {
				let page = match self.rpc.incoming_trc20(address, &self.usdt_contract, address_from, self.max_transfers).await {
					Ok(page) => page,
					Err(err) => {
						// One address's fetch failing MUST NOT abort the whole cycle and freeze the rail
						// for every other user (a `?` here would). Skip just this address; a transiently
						// failed one is re-scanned within the LOOKBACK_MS window next cycle.
						warn!(address, "tron deposit watcher: address scan failed, skipping this address this cycle: {err}");
						break;
					}
				};
				for transfer in &page.transfers {
					self.credit(*user, network, transfer).await?;
					high_watermark = high_watermark.max(transfer.block_timestamp);
				}
				if page.raw_len < self.max_transfers as usize {
					break;
				}
				if page.max_timestamp <= address_from {
					// A full page whose rows all share one timestamp, so time-paging can't advance
					// within THIS address. The fetched page was credited; break to the NEXT address
					// rather than `return`ing out of the whole cycle, which would starve every later
					// address forever (the cursor never advances, so each cycle re-hits this same page
					// first). The LOOKBACK_MS window re-scans this address next cycle; credit is
					// idempotent by tx id.
					warn!(address, "tron deposit watcher: full page without time progress — skipping this address for the cycle");
					break;
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
		// Defence in depth: the scan already filters by `contract_address`, but a node that ignored
		// the filter must never credit a non-USDT token as USDT. Skip ONLY on a NON-EMPTY, clearly
		// different token: `token_info.address` can be absent (decodes to ""), and dropping those —
		// or advancing the watermark past them — would silently lose a real deposit the server filter
		// already vouched for. An empty token ⇒ trust the filter; compare case-insensitively so a
		// non-canonical `TRON_USDT_CONTRACT` casing doesn't reject every row (canonical base58 is the
		// documented form). Only a populated token that genuinely differs is skipped.
		if !transfer.token.is_empty() && !transfer.token.eq_ignore_ascii_case(&self.usdt_contract) {
			warn!(user = %user, tx = %transfer.transaction_id, token = %transfer.token, "tron deposit watcher: transfer contract does not match USDT — skipping");
			return Ok(());
		}
		let amount = Usdt::from_onchain(network, transfer.value).map_err(|e| WatcherError::Decode(e.to_string()))?;
		if amount.is_zero() {
			return Ok(()); // a legal but meaningless zero-value transfer — not a deposit.
		}
		// Disambiguate per recipient: `deposits.tx_ref` is a GLOBAL primary key, but one Tron
		// transaction can pay several of our derived addresses (an exchange batching TRC20
		// withdrawals, a multisend). Keying on the bare `transaction_id` would credit the first
		// recipient and silently drop the rest on the shared-key conflict. `to` is the queried
		// address (the feed is scanned per-address with `only_to`), so `{tx}:{to}` is unique per
		// credited recipient and stable across re-scans. NOTE: this is weaker than BEP20's
		// `{tx}:{logindex}` in one pathological case — two TRC20 Transfer events to the SAME address
		// in ONE transaction (a looping contract) share `{tx}:{to}` and only the first is credited.
		// The `/v1` trc20 feed exposes no per-event index to append; a normal wallet deposit is one
		// Transfer per tx, so this is an accepted edge, not a real deposit path.
		let tx_ref = TxRef::parse(&format!("{}:{}", transfer.transaction_id, transfer.to)).map_err(|e| WatcherError::Decode(e.to_string()))?;
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
