//! On-chain TON withdrawal confirmation watcher — auto-settles broadcast jetton
//! withdrawals **only on positive proof the USDT left**.
//!
//! The TON sibling of [`withdrawal_watcher`](super::withdrawal_watcher). A TON withdrawal
//! is sent from the **treasury** v4R2 wallet, whose `seqno` strictly increments by one per
//! processed external message. A seqno advance proves the wallet *processed a message* at
//! that seqno — it does NOT prove the internal TEP-74 jetton transfer succeeded: a bounced
//! transfer (recipient can't accept, our jetton wallet short) advances the seqno just the
//! same while the USDT returns to the treasury. So the watcher never settles on a bare
//! seqno advance; it settles only when the indexer also shows a matching, non-aborted
//! *outgoing* jetton transfer of the expected amount (the mirror of the deposit path). The
//! settle then records that transfer's real transaction hash, and runs the same row-locked,
//! idempotent [`settle_withdrawal`](crate::application::withdrawals::settle_withdrawal) an
//! operator's `SettleWithdrawal` would.
//!
//! Split by safety, like the reaper and the BEP20 watcher:
//!   - **seqno advanced AND a matching outgoing transfer exists → AUTO-SETTLE** with the
//!     real tx hash. The USDT provably left.
//!   - **seqno advanced but NO matching outgoing transfer → DO NOT SETTLE.** Either the send
//!     bounced (funds back in treasury; the user's reserve stays held, recoverable by an
//!     operator `fail`) or the indexer just hasn't caught up — both are safe to wait on. The
//!     reaper alerts if it stays stuck. This is where the old "settle on seqno advance"
//!     silently lost a bounced withdrawal.
//!   - **seqno == the broadcast's seqno → next in line.** If the stored send has expired
//!     before its turn (which would freeze the strictly-sequential pipeline forever, since
//!     re-sending expired bytes can never land), ask custody to re-sign it at the SAME seqno
//!     with a fresh window ([`TonCustody::resign_stuck`]); otherwise re-broadcast the stored
//!     bytes so an out-of-order send lands when its turn comes.
//!   - **seqno < the broadcast's seqno → wait.** A queued future seqno — not its turn.
//!
//! Read-mostly; never touches TigerBeetle — money is still written last, in the relay.

use std::{collections::HashSet, sync::Arc, time::Duration};

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
	config::TonConfig,
	infrastructure::{
		ton_custody::TonCustody,
		ton_rpc::{JettonDeposit, TonRpc},
	},
	ports::{WithdrawalRepository, custody::BroadcastRequest},
};

/// How far back (unix seconds, before a send's stored `valid_until`) to look for the
/// matching outgoing transfer — generous slack over the signed validity window so the
/// indexer's decoded transfer is always in range.
const SETTLE_LOOKBACK_SECS: i64 = 3600;
/// Cap on outgoing transfers pulled per scan — well above any realistic in-flight depth.
const OUTGOING_LIMIT: u32 = 256;

pub struct TonWithdrawalWatcher {
	pool: PgPool,
	rpc: TonRpc,
	custody: Arc<TonCustody>,
	usdt_master: String,
	withdrawals: Arc<dyn WithdrawalRepository>,
	relay: Arc<Notify>,
	poll: Duration,
}

impl TonWithdrawalWatcher {
	pub fn new(pool: PgPool, custody: Arc<TonCustody>, withdrawals: Arc<dyn WithdrawalRepository>, relay: Arc<Notify>, config: &TonConfig) -> Self {
		Self {
			pool,
			rpc: TonRpc::new(config.api_url.clone(), config.api_key.clone()),
			custody,
			usdt_master: config.usdt_master.clone(),
			withdrawals,
			relay,
			poll: Duration::from_secs(config.poll_secs),
		}
	}

	pub async fn run(self, shutdown: CancellationToken) {
		info!("ton withdrawal watcher: settling jetton withdrawals on a proven outgoing transfer");
		loop {
			if let Err(err) = self.scan_once().await {
				warn!("ton withdrawal watcher: scan cycle failed, retrying next poll: {err}");
			}
			tokio::select! {
				() = shutdown.cancelled() => {
					info!("ton withdrawal watcher: shutdown requested — stopping");
					return;
				}
				() = tokio::time::sleep(self.poll) => {}
			}
		}
	}

	async fn scan_once(&self) -> Result<(), WatcherError> {
		let pending = self.pending_broadcasts().await?;
		if pending.is_empty() {
			return Ok(()); // nothing in flight — skip the seqno read entirely.
		}
		let treasury = self.custody.treasury_address().await.map_err(|e| WatcherError::Custody(e.to_string()))?;
		let seqno = self.rpc.seqno(&treasury).await.map_err(|e| WatcherError::Rpc(e.to_string()))?;

		// One outgoing-transfer read covers every settle-candidate this scan. Only fetch when
		// something has actually advanced past its signed seqno.
		let advanced_from = pending.iter().filter(|p| seqno > p.signed_seqno).map(|p| p.valid_until).min();
		let outgoing = match advanced_from {
			Some(oldest) => {
				let start = oldest.saturating_sub(SETTLE_LOOKBACK_SECS).max(0) as u64;
				self.rpc
					.outgoing_jetton_transfers(&treasury, &self.usdt_master, start, OUTGOING_LIMIT)
					.await
					.map_err(|e| WatcherError::Rpc(e.to_string()))?
			}
			None => Vec::new(),
		};

		// Attribute each outgoing transfer to at most one withdrawal so two same-amount sends
		// don't both claim one on-chain transfer.
		let mut claimed = HashSet::new();
		for pending in &pending {
			match seqno.cmp(&pending.signed_seqno) {
				// The treasury processed the message at this seqno — but only a matching
				// non-aborted outgoing transfer proves the USDT actually left. Absent that, the
				// send bounced (or isn't indexed yet): leave it `processing` (reserve held,
				// recoverable), never settle it into a phantom disbursement.
				std::cmp::Ordering::Greater => match match_outgoing(pending.net_onchain, &outgoing, &mut claimed) {
					Some(tx_hash) => self.settle(pending.withdrawal_id, &tx_hash).await,
					None => warn!(
						withdrawal_id = %pending.withdrawal_id,
						seqno = pending.signed_seqno,
						"ton withdrawal watcher: treasury seqno advanced but no matching outgoing USDT transfer — NOT settling (bounced or not-yet-indexed); reaper/operator backstops"
					),
				},
				// This withdrawal is next in line. Re-sign it if the stored send has expired
				// (else the pipeline freezes on un-landable bytes), otherwise re-broadcast.
				std::cmp::Ordering::Equal => self.advance_next_in_line(pending).await,
				// A queued future seqno — wait for the earlier ones to land first.
				std::cmp::Ordering::Less => {}
			}
		}
		Ok(())
	}

	/// Push the next-in-line send forward: recover it if its stored message has provably
	/// expired (re-sign at the same seqno, unfreezing the queue), else re-broadcast the stored
	/// bytes so a send first issued out of order lands now that its turn has come.
	async fn advance_next_in_line(&self, pending: &PendingBroadcast) {
		match self.custody.resign_stuck(&pending.request).await {
			Ok(true) => {} // re-signed at the same seqno + re-broadcast (logged by custody)
			Ok(false) => self.rebroadcast(pending).await,
			Err(err) => error!(withdrawal_id = %pending.withdrawal_id, "ton withdrawal watcher: could not recover a stuck expired send (needs intervention): {err}"),
		}
	}

	async fn rebroadcast(&self, pending: &PendingBroadcast) {
		match self.rpc.send_message(&pending.raw_tx).await {
			Ok(()) => info!(withdrawal_id = %pending.withdrawal_id, seqno = pending.signed_seqno, "ton withdrawal watcher: re-broadcast the next-in-line send"),
			Err(err) => warn!(withdrawal_id = %pending.withdrawal_id, "ton withdrawal watcher: re-broadcast failed (will retry next poll): {err}"),
		}
	}

	async fn settle(&self, withdrawal_id: Uuid, tx_hash: &str) {
		let tx_ref = match TxRef::parse(tx_hash) {
			Ok(tx_ref) => tx_ref,
			Err(err) => {
				warn!(%withdrawal_id, "ton withdrawal watcher: outgoing tx hash is unparseable, skipping: {err}");
				return;
			}
		};
		match settle_withdrawal(self.withdrawals.as_ref(), &self.relay, WithdrawalId::from_raw(withdrawal_id), tx_ref).await {
			Ok(_) => info!(%withdrawal_id, %tx_hash, "ton withdrawal watcher: proven outgoing USDT transfer — settled"),
			Err(err) => warn!(%withdrawal_id, "ton withdrawal watcher: could not settle confirmed withdrawal (will retry next poll): {err}"),
		}
	}

	/// The `processing` TON withdrawals we have broadcast, with the net amount and stored send
	/// details needed to prove settlement and to recover a stuck send. The withdrawal *state*
	/// is the source of truth — a settled one leaves the set, so a re-settle is never attempted.
	async fn pending_broadcasts(&self) -> Result<Vec<PendingBroadcast>, WatcherError> {
		let rows = sqlx::query_as::<_, PendingRow>(
			"SELECT b.withdrawal_id, b.nonce AS seqno, b.expiration AS valid_until, b.raw_tx, w.address, w.amount, w.fee FROM withdrawal_broadcasts b \
			 JOIN withdrawals w ON w.id = b.withdrawal_id WHERE b.network = 'ton' AND w.state = 'processing'",
		)
		.fetch_all(&self.pool)
		.await
		.map_err(|e| WatcherError::Db(e.to_string()))?;
		let mut pending = Vec::with_capacity(rows.len());
		for row in rows {
			let withdrawal_id = row.withdrawal_id;
			match PendingBroadcast::build(row) {
				Ok(p) => pending.push(p),
				Err(err) => warn!(%withdrawal_id, "ton withdrawal watcher: skipping a malformed pending broadcast: {err}"),
			}
		}
		Ok(pending)
	}
}

/// The raw join row backing a [`PendingBroadcast`] (`withdrawal_broadcasts` ⋈ `withdrawals`).
#[derive(sqlx::FromRow)]
struct PendingRow {
	withdrawal_id: Uuid,
	seqno: Option<i64>,
	valid_until: Option<i64>,
	raw_tx: String,
	address: String,
	amount: String,
	fee: String,
}

/// One `processing` TON withdrawal's stored broadcast + the data needed to prove its
/// settlement and to recover it if stuck.
struct PendingBroadcast {
	withdrawal_id: Uuid,
	/// The seqno the broadcast was signed at.
	signed_seqno: u64,
	/// The stored `valid_until` (unix seconds) — the settle-lookback anchor.
	valid_until: i64,
	/// The base64 BoC, re-broadcast when this send is next in line and still live.
	raw_tx: String,
	/// The net USDT in on-chain (6-dp) base units — the outgoing-transfer match key.
	net_onchain: u128,
	/// A reconstructed broadcast request, for a re-sign recovery.
	request: BroadcastRequest,
}

impl PendingBroadcast {
	fn build(row: PendingRow) -> Result<Self, String> {
		let gross = Usdt::from_base_units(row.amount.parse::<u128>().map_err(|_| "malformed amount")?);
		let fee = Usdt::from_base_units(row.fee.parse::<u128>().map_err(|_| "malformed fee")?);
		let net = gross.checked_sub(fee).ok_or("fee exceeds amount")?;
		let net_onchain = net.to_onchain(Network::Ton).map_err(|e| e.to_string())?;
		let address = WalletAddress::parse(Network::Ton, &row.address).map_err(|e| e.to_string())?;
		Ok(Self {
			withdrawal_id: row.withdrawal_id,
			signed_seqno: row.seqno.unwrap_or(0).max(0) as u64,
			valid_until: row.valid_until.unwrap_or(0),
			raw_tx: row.raw_tx,
			net_onchain,
			request: BroadcastRequest {
				withdrawal_id: row.withdrawal_id,
				network: Network::Ton,
				address,
				amount: net,
			},
		})
	}
}

/// Find an unclaimed, non-aborted outgoing transfer of exactly `net` on-chain units and
/// claim it (so two same-amount sends can't both attribute to one transfer). The indexer
/// only surfaces transfers whose transaction was not aborted, so a match is positive proof
/// the USDT left. Pure — unit-tested without an indexer.
fn match_outgoing(net: u128, transfers: &[JettonDeposit], claimed: &mut HashSet<String>) -> Option<String> {
	let hit = transfers.iter().find(|t| t.amount == net && !claimed.contains(&t.tx_hash))?;
	claimed.insert(hit.tx_hash.clone());
	Some(hit.tx_hash.clone())
}

#[derive(Debug, thiserror::Error)]
enum WatcherError {
	#[error("rpc: {0}")]
	Rpc(String),
	#[error("custody: {0}")]
	Custody(String),
	#[error("db: {0}")]
	Db(String),
}

#[cfg(test)]
mod tests {
	use super::*;

	fn transfer(hash: &str, amount: u128) -> JettonDeposit {
		JettonDeposit {
			tx_hash: hash.to_owned(),
			amount,
			now: 0,
		}
	}

	#[test]
	fn matches_by_amount_and_claims_each_transfer_once() {
		let transfers = vec![transfer("a", 5_000_000), transfer("b", 5_000_000), transfer("c", 9_000_000)];
		let mut claimed = HashSet::new();
		// Two same-amount withdrawals consume the two matching transfers, not the same one.
		assert_eq!(match_outgoing(5_000_000, &transfers, &mut claimed), Some("a".to_owned()));
		assert_eq!(match_outgoing(5_000_000, &transfers, &mut claimed), Some("b".to_owned()));
		// A third same-amount withdrawal has no unclaimed transfer left — not settleable yet.
		assert_eq!(match_outgoing(5_000_000, &transfers, &mut claimed), None);
		// A different amount matches its own transfer.
		assert_eq!(match_outgoing(9_000_000, &transfers, &mut claimed), Some("c".to_owned()));
	}

	#[test]
	fn no_matching_amount_is_never_settled() {
		let transfers = vec![transfer("a", 5_000_000)];
		let mut claimed = HashSet::new();
		// A bounced send leaves no outgoing transfer of the expected amount → no settle.
		assert_eq!(match_outgoing(7_000_000, &transfers, &mut claimed), None);
	}
}
