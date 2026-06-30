//! Real TON custody — the relay's "broadcast this jetton withdrawal" seam for the TON
//! rail. The TON sibling of [`ChainCustody`](super::custody::ChainCustody).
//!
//! It signs the withdrawal's TEP-74 jetton (USDT) transfer via the signer (the key never
//! leaves there) and POSTs the BoC to toncenter. **Crash-safety / no double-spend:** the
//! signed message is persisted (`withdrawal_broadcasts`, keyed by `withdrawal_id`) BEFORE
//! it is sent, so an at-least-once relay re-delivery re-broadcasts the SAME bytes (same
//! seqno) rather than signing a new one. Re-sending is safe by construction: the v4R2
//! wallet requires the message's `seqno` to exactly equal its stored counter, so once it
//! has advanced, a stale re-broadcast is silently rejected on-chain (it does nothing).
//!
//! **Seqno sequencing.** Each withdrawal is signed at `max(chain_seqno, max(stored ton
//! seqno) + 1)` — the same monotonic rule as the EVM nonce, so back-to-back withdrawals
//! get distinct, ordered seqnos and never collide even before the prior lands. Unlike EVM
//! (where a future nonce queues in the mempool), the wallet drops an out-of-order seqno, so
//! the [`ton_withdrawal_watcher`](super::ton_withdrawal_watcher) re-broadcasts each stored
//! send when its turn comes (chain seqno == its seqno) to drain the queue in order.
//!
//! The on-chain SETTLE (reducing the ledger's rail custody) is the separate confirmation
//! watcher; this adapter only gets the bytes onto the chain.

use async_trait::async_trait;
use domain::money::Network;
use evbanking_auth::ServiceTokenSource;
use evbanking_contracts::signer::v1::{ProvisionAddressRequest, SignJettonTransferRequest, signer_service_client::SignerServiceClient};
use sqlx::PgPool;
use tokio::sync::OnceCell;
use tonic::{Request, transport::Channel};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
	config::TonConfig,
	infrastructure::ton_rpc::{RpcError, TonRpc},
	ports::custody::{BroadcastRequest, Custody, CustodyError},
};

/// Seconds a signed external message stays valid (`valid_until = now + this`). Generous so
/// a queued send reaches its seqno turn before expiring; deeper queues are an operator
/// residual (the reaper alerts on a stuck `processing` withdrawal).
const VALID_WINDOW_SECS: u64 = 300;

pub struct TonCustody {
	pool: PgPool,
	rpc: TonRpc,
	signer: SignerServiceClient<Channel>,
	service_token: Option<ServiceTokenSource>,
	usdt_master: String,
	forward_ton_amount: u64,
	msg_value: u64,
	/// The treasury hot wallet's TON address (the withdrawal source), resolved once and
	/// cached. Funds — USDT to send, TON for gas — are deposited here out-of-band.
	treasury: OnceCell<String>,
	/// The treasury's USDT jetton wallet (the internal-message destination), resolved once
	/// from the indexer and cached.
	treasury_jetton_wallet: OnceCell<String>,
}

impl TonCustody {
	pub fn new(pool: PgPool, channel: Channel, service_token: Option<ServiceTokenSource>, config: &TonConfig) -> Self {
		Self {
			pool,
			rpc: TonRpc::new(config.api_url.clone(), config.api_key.clone()),
			signer: SignerServiceClient::new(channel),
			service_token,
			usdt_master: config.usdt_master.clone(),
			forward_ton_amount: config.forward_ton_amount,
			msg_value: config.msg_value,
			treasury: OnceCell::new(),
			treasury_jetton_wallet: OnceCell::new(),
		}
	}

	/// The treasury's TON address, resolved once via `ProvisionAddress` (the nil user id) and
	/// cached. Public so the composition root can resolve + log it at boot — the operator
	/// funds this address (USDT for liquidity, TON for gas) before withdrawals can settle.
	pub async fn treasury_address(&self) -> Result<String, CustodyError> {
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
					.map_err(|s| CustodyError::Unavailable(format!("resolve treasury address: {}", s.message())))?
					.into_inner();
				if response.address_kind != "derived" {
					return Err(CustodyError::Rejected(format!("treasury address is not fundable (kind={})", response.address_kind)));
				}
				info!(treasury = %response.address, "ton custody: treasury hot wallet — fund it with USDT (liquidity) + TON (gas)");
				Ok(response.address)
			})
			.await
			.cloned()
	}

	/// The treasury's USDT jetton wallet — the destination of the internal message that
	/// carries the TEP-74 transfer. Resolved from the indexer; `None` (treasury never
	/// received USDT) is a fundable-liquidity problem parked for the operator.
	async fn treasury_jetton_wallet(&self, treasury: &str) -> Result<String, CustodyError> {
		self.treasury_jetton_wallet
			.get_or_try_init(|| async {
				let wallet = self
					.rpc
					.jetton_wallet(treasury, &self.usdt_master)
					.await
					.map_err(read_err)?
					.ok_or_else(|| CustodyError::Rejected("treasury has no USDT jetton wallet yet — fund the treasury with USDT".to_owned()))?;
				Ok(wallet.address)
			})
			.await
			.cloned()
	}

	async fn stored_tx(&self, withdrawal_id: Uuid) -> Result<Option<String>, CustodyError> {
		sqlx::query_scalar::<_, String>("SELECT raw_tx FROM withdrawal_broadcasts WHERE withdrawal_id = $1")
			.bind(withdrawal_id)
			.fetch_optional(&self.pool)
			.await
			.map_err(db_unavailable)
	}

	async fn store_tx(&self, withdrawal_id: Uuid, seqno: u64, valid_until: u32, raw_tx: &str, tx_hash: &str) -> Result<(), CustodyError> {
		sqlx::query(
			"INSERT INTO withdrawal_broadcasts (withdrawal_id, network, nonce, expiration, raw_tx, tx_hash) \
			 VALUES ($1, 'ton', $2, $3, $4, $5) ON CONFLICT (withdrawal_id) DO NOTHING",
		)
		.bind(withdrawal_id)
		.bind(seqno as i64)
		.bind(valid_until as i64)
		.bind(raw_tx)
		.bind(tx_hash)
		.execute(&self.pool)
		.await
		.map_err(db_unavailable)?;
		Ok(())
	}

	/// The next treasury seqno: `max(chain seqno, highest stored ton seqno + 1)` — monotonic
	/// even if several withdrawals are in flight before the first lands, and it catches up to
	/// the chain after a restart. Scoped to `network = 'ton'`.
	async fn next_seqno(&self, treasury: &str) -> Result<u64, CustodyError> {
		let chain = self.rpc.seqno(treasury).await.map_err(read_err)?;
		let local_max: Option<i64> = sqlx::query_scalar("SELECT MAX(nonce) FROM withdrawal_broadcasts WHERE network = 'ton'")
			.fetch_one(&self.pool)
			.await
			.map_err(db_unavailable)?;
		let local_next = local_max.map(|n| n as u64 + 1).unwrap_or(0);
		Ok(chain.max(local_next))
	}

	/// Sign the withdrawal's jetton transfer from the treasury key via the signer.
	async fn sign(&self, request: &BroadcastRequest, treasury: &str, treasury_jetton_wallet: &str, amount: u128, seqno: u64, valid_until: u32) -> Result<(String, String), CustodyError> {
		let mut signer_request = Request::new(SignJettonTransferRequest {
			from_user_id: String::new(), // empty ⇒ treasury hot wallet
			network: "ton".to_owned(),
			our_jetton_wallet: treasury_jetton_wallet.to_owned(),
			to_address: request.address.as_str().to_owned(),
			amount: amount.to_string(),
			response_destination: treasury.to_owned(),
			forward_ton_amount: self.forward_ton_amount,
			msg_value: self.msg_value,
			seqno,
			valid_until,
			is_testnet: false,
			wallet_version: String::new(),
		});
		if let Some(token) = &self.service_token {
			signer_request = token.authorize(signer_request);
		}
		let response = self.signer.clone().sign_jetton_transfer(signer_request).await.map_err(|s| match s.code() {
			tonic::Code::Unavailable | tonic::Code::DeadlineExceeded => CustodyError::Unavailable(format!("signer: {}", s.message())),
			_ => CustodyError::Rejected(format!("signer: {}", s.message())),
		})?;
		let response = response.into_inner();
		Ok((response.signed_boc, response.msg_hash))
	}

	/// Submit a signed BoC. A transport failure is retryable (nothing reached the chain); a
	/// toncenter rejection parks the withdrawal for intervention.
	async fn submit(&self, boc: &str, rebroadcast: bool) -> Result<(), CustodyError> {
		match self.rpc.send_message(boc).await {
			Ok(()) => {
				info!(rebroadcast, "ton custody: broadcast withdrawal message");
				Ok(())
			}
			Err(RpcError::Transport(detail)) => Err(CustodyError::Unavailable(detail)),
			Err(RpcError::Rpc(msg)) => {
				warn!(reason = %msg, "ton custody: toncenter rejected the message — parking");
				Err(CustodyError::Rejected(msg))
			}
		}
	}
}

#[async_trait]
impl Custody for TonCustody {
	async fn broadcast(&self, request: &BroadcastRequest) -> Result<(), CustodyError> {
		debug_assert!(
			matches!(request.network, Network::Ton),
			"TonCustody is the TON adapter; the registry must not route {} here",
			request.network
		);
		// Idempotent: if we already signed+stored a message for this withdrawal, re-send THOSE
		// exact bytes rather than signing a new one (the wallet's strict-seqno rule makes a
		// stale re-send a no-op; no second seqno can ever go out).
		if let Some(boc) = self.stored_tx(request.withdrawal_id).await? {
			return self.submit(&boc, true).await;
		}

		let treasury = self.treasury_address().await?;
		let treasury_jetton_wallet = self.treasury_jetton_wallet(&treasury).await?;
		let amount = request
			.amount
			.to_onchain(Network::Ton)
			.map_err(|e| CustodyError::Rejected(format!("amount not representable on TON: {e}")))?;
		let seqno = self.next_seqno(&treasury).await?;
		let valid_until = (now_unix() + VALID_WINDOW_SECS) as u32;
		let (boc, msg_hash) = self.sign(request, &treasury, &treasury_jetton_wallet, amount, seqno, valid_until).await?;

		// Persist BEFORE broadcasting — a crash after this re-broadcasts THIS message (same
		// seqno), never a freshly-signed one.
		self.store_tx(request.withdrawal_id, seqno, valid_until, &boc, &msg_hash).await?;
		self.submit(&boc, false).await
	}
}

fn now_unix() -> u64 {
	std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// A read-path RPC failure (seqno/jetton-wallet) is always retryable — nothing was sent.
fn read_err(err: RpcError) -> CustodyError {
	CustodyError::Unavailable(err.to_string())
}

fn db_unavailable(err: sqlx::Error) -> CustodyError {
	CustodyError::Unavailable(format!("custody db: {err}"))
}
