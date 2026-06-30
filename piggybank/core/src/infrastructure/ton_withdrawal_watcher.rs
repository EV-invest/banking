//! On-chain TON withdrawal confirmation watcher — auto-settles broadcast jetton
//! withdrawals.
//!
//! The TON sibling of [`withdrawal_watcher`](super::withdrawal_watcher). A TON withdrawal
//! is sent from the **treasury** v4R2 wallet, whose `seqno` strictly increments by one per
//! processed external message. So "did this withdrawal land?" is answered categorically by
//! the treasury's `seqno` advancing past the seqno the broadcast was signed at — no
//! N-confirmations arithmetic (TON finality is fast and categorical; toncenter only returns
//! committed transactions). Once advanced, we call
//! [`settle_withdrawal`](crate::application::withdrawals::settle_withdrawal) — the same
//! row-locked, idempotent command an operator's `SettleWithdrawal` runs.
//!
//! Split by safety, like the reaper and the BEP20 watcher:
//!   - **chain seqno > the broadcast's seqno → AUTO-SETTLE.** The treasury executed the
//!     send (seqno only advances by processing the message at that seqno, and we are the
//!     sole key holder).
//!   - **chain seqno == the broadcast's seqno → RE-BROADCAST.** This withdrawal is next in
//!     line; its stored BoC is re-sent so a send first issued out of order (a later
//!     withdrawal signed at a future seqno before its turn, which the wallet drops) actually
//!     lands when its turn comes. Re-sending the same bytes is idempotent.
//!   - **chain seqno < the broadcast's seqno → wait.** A queued future seqno — not its turn.
//!
//! This re-broadcast is what lets the custody hand out distinct, monotonic seqnos to
//! back-to-back withdrawals (so they never collide) while the wallet still requires strict
//! in-order processing. Residuals (operator/reaper-backstopped, not auto-handled): a queue
//! deeper than the signed `valid_until` window, and the re-sign of an expired-and-unlanded
//! send.
//!
//! Residual (documented, not auto-handled): a *bounced* jetton transfer (e.g. the recipient
//! cannot accept it) still advances the treasury seqno, so it settles here even though the
//! USDT returned. That is the same "our own transfer didn't complete" money decision the
//! BEP20 watcher leaves to an operator — surfaced via reconciliation, never auto-refunded.
//! Read-mostly; never touches TigerBeetle — money is still written last, in the relay.

use std::{sync::Arc, time::Duration};

use domain::{money::TxRef, withdrawals::WithdrawalId};
use evbanking_auth::ServiceTokenSource;
use evbanking_contracts::signer::v1::{ProvisionAddressRequest, signer_service_client::SignerServiceClient};
use sqlx::PgPool;
use tokio::sync::{Notify, OnceCell};
use tokio_util::sync::CancellationToken;
use tonic::{Request, transport::Channel};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{application::withdrawals::settle_withdrawal, config::TonConfig, infrastructure::ton_rpc::TonRpc, ports::WithdrawalRepository};

pub struct TonWithdrawalWatcher {
	pool: PgPool,
	rpc: TonRpc,
	signer: SignerServiceClient<Channel>,
	service_token: Option<ServiceTokenSource>,
	withdrawals: Arc<dyn WithdrawalRepository>,
	relay: Arc<Notify>,
	/// The treasury hot wallet's TON address (the withdrawal source), resolved once via the
	/// signer and cached — all TON withdrawals share its seqno.
	treasury: OnceCell<String>,
	poll: Duration,
}

impl TonWithdrawalWatcher {
	pub fn new(pool: PgPool, channel: Channel, service_token: Option<ServiceTokenSource>, withdrawals: Arc<dyn WithdrawalRepository>, relay: Arc<Notify>, config: &TonConfig) -> Self {
		Self {
			pool,
			rpc: TonRpc::new(config.api_url.clone(), config.api_key.clone()),
			signer: SignerServiceClient::new(channel),
			service_token,
			withdrawals,
			relay,
			treasury: OnceCell::new(),
			poll: Duration::from_secs(config.poll_secs),
		}
	}

	pub async fn run(self, shutdown: CancellationToken) {
		info!("ton withdrawal watcher: confirming broadcast jetton withdrawals via treasury seqno");
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
		let treasury = self.treasury_address().await?;
		let seqno = self.rpc.seqno(&treasury).await.map_err(|e| WatcherError::Rpc(e.to_string()))?;
		for pending in pending {
			match seqno.cmp(&pending.signed_seqno) {
				// The treasury processed the message at this seqno (the only way it advances)
				// — the send executed; settle.
				std::cmp::Ordering::Greater => self.settle(pending.withdrawal_id, &pending.tx_hash).await,
				// This withdrawal is next in line — (re)broadcast its stored BoC so a send
				// first issued out of order actually lands now that its turn has come.
				std::cmp::Ordering::Equal => self.rebroadcast(&pending).await,
				// A queued future seqno — wait for the earlier ones to land first.
				std::cmp::Ordering::Less => {}
			}
		}
		Ok(())
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
				warn!(%withdrawal_id, "ton withdrawal watcher: stored tx hash is unparseable, skipping: {err}");
				return;
			}
		};
		match settle_withdrawal(self.withdrawals.as_ref(), &self.relay, WithdrawalId::from_raw(withdrawal_id), tx_ref).await {
			Ok(_) => info!(%withdrawal_id, %tx_hash, "ton withdrawal watcher: treasury seqno advanced — settled"),
			Err(err) => warn!(%withdrawal_id, "ton withdrawal watcher: could not settle confirmed withdrawal (will retry next poll): {err}"),
		}
	}

	/// The `processing` TON withdrawals we have broadcast. The withdrawal *state* is the
	/// source of truth — a settled one leaves the set, so a re-settle is never attempted.
	async fn pending_broadcasts(&self) -> Result<Vec<PendingBroadcast>, WatcherError> {
		let rows: Vec<(Uuid, Option<i64>, String, String)> = sqlx::query_as(
			"SELECT b.withdrawal_id, b.nonce, b.tx_hash, b.raw_tx FROM withdrawal_broadcasts b \
			 JOIN withdrawals w ON w.id = b.withdrawal_id WHERE b.network = 'ton' AND w.state = 'processing'",
		)
		.fetch_all(&self.pool)
		.await
		.map_err(|e| WatcherError::Db(e.to_string()))?;
		Ok(rows
			.into_iter()
			.map(|(withdrawal_id, seqno, tx_hash, raw_tx)| PendingBroadcast {
				withdrawal_id,
				signed_seqno: seqno.unwrap_or(0).max(0) as u64,
				tx_hash,
				raw_tx,
			})
			.collect())
	}

	/// The treasury's TON address, resolved once via `ProvisionAddress` (the nil user id) and
	/// cached. A transient failure leaves the cell empty so a later cycle retries.
	async fn treasury_address(&self) -> Result<String, WatcherError> {
		self.treasury
			.get_or_try_init(|| async {
				let mut request = Request::new(ProvisionAddressRequest {
					user_id: Uuid::nil().to_string(),
					network: "ton".to_owned(),
				});
				if let Some(token) = &self.service_token {
					request = token.authorize(request);
				}
				let response = self
					.signer
					.clone()
					.provision_address(request)
					.await
					.map_err(|s| WatcherError::Signer(format!("resolve treasury address: {}", s.message())))?
					.into_inner();
				Ok(response.address)
			})
			.await
			.cloned()
	}
}

/// One `processing` TON withdrawal's stored broadcast.
struct PendingBroadcast {
	withdrawal_id: Uuid,
	/// The seqno the broadcast was signed at.
	signed_seqno: u64,
	/// The external-message hash — the settle tx ref.
	tx_hash: String,
	/// The base64 BoC, re-broadcast when this send is next in line.
	raw_tx: String,
}

#[derive(Debug, thiserror::Error)]
enum WatcherError {
	#[error("rpc: {0}")]
	Rpc(String),
	#[error("signer: {0}")]
	Signer(String),
	#[error("db: {0}")]
	Db(String),
}
