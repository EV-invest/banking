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

use std::{
	collections::{HashMap, HashSet},
	sync::Arc,
	time::Duration,
};

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

		// A seqno advance only proves the wallet processed *a* message — a bounce advances it
		// too — so a send settles only against a matching non-aborted outgoing transfer.
		let advanced: Vec<&PendingBroadcast> = pending.iter().filter(|p| seqno > p.signed_seqno).collect();
		let next_in_line = pending.iter().filter(|p| seqno == p.signed_seqno);

		if !advanced.is_empty() {
			let start = advanced.iter().map(|p| p.valid_until).min().unwrap_or(0).saturating_sub(SETTLE_LOOKBACK_SECS).max(0) as u64;
			let outgoing = self
				.rpc
				.outgoing_jetton_transfers(&treasury, &self.usdt_master, start, OUTGOING_LIMIT)
				.await
				.map_err(|e| WatcherError::Rpc(e.to_string()))?;
			// Durable cross-scan dedup: a transfer already recorded as some withdrawal's settle
			// tx must never be re-attributed on a later scan (the per-scan claim set can't see
			// across scans, and the transfer lingers in the lookback window after the owner
			// settles and drops out of `pending`).
			let already_used = self.used_tx_refs(&outgoing).await?;
			let settlements = plan_settlements(&advanced, &outgoing, &already_used);
			for p in &advanced {
				match settlements.get(&p.withdrawal_id) {
					Some(tx_hash) => self.settle(p.withdrawal_id, tx_hash).await,
					None => warn!(
						withdrawal_id = %p.withdrawal_id,
						seqno = p.signed_seqno,
						"ton withdrawal watcher: treasury seqno advanced but no unambiguous matching outgoing USDT transfer — NOT settling (bounced, not-yet-indexed, or a same-amount sibling is ambiguous); reaper/operator backstops"
					),
				}
			}
		}
		// Next-in-line sends: recover if expired (unfreeze the queue), else re-broadcast.
		for pending in next_in_line {
			self.advance_next_in_line(pending).await;
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

	/// Which of `outgoing`'s tx hashes are ALREADY recorded as some TON withdrawal's settle
	/// `tx_ref` — so a transfer can never settle a second withdrawal on a later scan. The
	/// durable half of the dedup (the per-scan grouping in [`plan_settlements`] is the other).
	async fn used_tx_refs(&self, outgoing: &[JettonDeposit]) -> Result<HashSet<String>, WatcherError> {
		if outgoing.is_empty() {
			return Ok(HashSet::new());
		}
		let hashes: Vec<String> = outgoing.iter().map(|t| t.tx_hash.clone()).collect();
		let rows: Vec<(String,)> = sqlx::query_as("SELECT tx_ref FROM withdrawals WHERE network = 'ton' AND tx_ref = ANY($1)")
			.bind(&hashes)
			.fetch_all(&self.pool)
			.await
			.map_err(|e| WatcherError::Db(e.to_string()))?;
		Ok(rows.into_iter().map(|(t,)| t).collect())
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

/// Decide which advanced withdrawals may settle, and against which outgoing transfer.
///
/// Amount is the only identity we have (the indexer feed carries no destination we can
/// compare without decoding TON addresses, which core avoids). So a settlement is only safe
/// when it is **unambiguous**: for each net-amount group, we settle its members only if there
/// are at least as many distinct, non-aborted, not-already-used outgoing transfers of that
/// amount as there are withdrawals in the group — i.e. every same-amount sibling has its own
/// landed transfer. If fewer transfers than siblings, at least one bounced and amount alone
/// can't say which, so **none** of the group settles (they wait for the operator/reaper). A
/// lone withdrawal of a unique amount settles as soon as its transfer appears. `already_used`
/// (transfers recorded on prior settlements) is excluded so nothing double-settles across
/// scans. Pure — unit-tested without an indexer.
fn plan_settlements(advanced: &[&PendingBroadcast], outgoing: &[JettonDeposit], already_used: &HashSet<String>) -> HashMap<Uuid, String> {
	let mut available: HashMap<u128, Vec<&str>> = HashMap::new();
	for t in outgoing {
		if t.amount == 0 || already_used.contains(&t.tx_hash) {
			continue;
		}
		available.entry(t.amount).or_default().push(&t.tx_hash);
	}
	let mut by_amount: HashMap<u128, Vec<Uuid>> = HashMap::new();
	for p in advanced {
		by_amount.entry(p.net_onchain).or_default().push(p.withdrawal_id);
	}
	let mut settlements = HashMap::new();
	for (amount, ids) in by_amount {
		let transfers = available.get(&amount).map(Vec::as_slice).unwrap_or(&[]);
		// Fewer landed transfers than same-amount siblings ⇒ ambiguous ⇒ settle none of them.
		if transfers.len() < ids.len() {
			continue;
		}
		for (id, tx) in ids.iter().zip(transfers.iter()) {
			settlements.insert(*id, (*tx).to_owned());
		}
	}
	settlements
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

	fn pending(net: u128) -> PendingBroadcast {
		let id = Uuid::new_v4();
		PendingBroadcast {
			withdrawal_id: id,
			signed_seqno: 0,
			valid_until: 0,
			raw_tx: String::new(),
			net_onchain: net,
			request: BroadcastRequest {
				withdrawal_id: id,
				network: Network::Ton,
				address: WalletAddress::parse(Network::Ton, "EQCD39VS5jcptHL8vMjEXrzGaRcCVYto7HUn4bpAOg8xqB2N").unwrap(),
				amount: Usdt::from_base_units(net),
			},
		}
	}

	#[test]
	fn a_unique_amount_settles_against_its_transfer() {
		let w = pending(9_000_000);
		let transfers = vec![transfer("c", 9_000_000)];
		let plan = plan_settlements(&[&w], &transfers, &HashSet::new());
		assert_eq!(plan.get(&w.withdrawal_id), Some(&"c".to_owned()));
	}

	#[test]
	fn two_same_amount_sends_that_both_landed_each_get_a_distinct_transfer() {
		let (a, b) = (pending(5_000_000), pending(5_000_000));
		let transfers = vec![transfer("t1", 5_000_000), transfer("t2", 5_000_000)];
		let plan = plan_settlements(&[&a, &b], &transfers, &HashSet::new());
		let (ta, tb) = (plan.get(&a.withdrawal_id).unwrap(), plan.get(&b.withdrawal_id).unwrap());
		assert_ne!(ta, tb, "each same-amount withdrawal settles against a DISTINCT transfer");
		assert_eq!(plan.len(), 2);
	}

	#[test]
	fn same_amount_with_a_bounce_settles_none_of_the_group() {
		// A landed (one transfer of the amount), B bounced (no transfer) — amount alone can't
		// say which of A/B the single transfer belongs to, so NEITHER settles. This is the bug
		// the review found: without this, the greedy match could settle the bounced one.
		let (a, b) = (pending(5_000_000), pending(5_000_000));
		let transfers = vec![transfer("t1", 5_000_000)];
		let plan = plan_settlements(&[&a, &b], &transfers, &HashSet::new());
		assert!(plan.is_empty(), "an ambiguous same-amount group with a bounce settles nobody");
	}

	#[test]
	fn an_already_used_transfer_is_never_re_attributed() {
		// The cross-scan case: A already settled against t1 (state now completed, gone from the
		// pending set); a later same-amount B whose send bounced must NOT re-grab t1.
		let b = pending(5_000_000);
		let transfers = vec![transfer("t1", 5_000_000)];
		let already_used = HashSet::from(["t1".to_owned()]);
		let plan = plan_settlements(&[&b], &transfers, &already_used);
		assert!(plan.is_empty(), "a transfer already recorded on a prior settlement is excluded");
	}

	#[test]
	fn a_bounced_send_with_no_transfer_never_settles() {
		let w = pending(7_000_000);
		let transfers = vec![transfer("a", 5_000_000)];
		let plan = plan_settlements(&[&w], &transfers, &HashSet::new());
		assert!(plan.is_empty(), "no outgoing transfer of the expected amount → no settle");
	}
}
