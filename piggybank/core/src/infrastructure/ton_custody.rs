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
use domain::{
	architecture::Gateway,
	money::{Network, Usdt},
};
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
	ports::custody::{BroadcastRequest, Custody, CustodyError, TreasuryFunding, format_native_units},
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
		sqlx::query_scalar::<_, String>("SELECT raw_tx FROM withdrawal_broadcasts WHERE withdrawal_id = $1 AND network = 'ton'")
			.bind(withdrawal_id)
			.fetch_optional(&self.pool)
			.await
			.map_err(db_unavailable)
	}

	/// The stored `(seqno, valid_until)` for a withdrawal's TON send, if any.
	async fn stored_seqno(&self, withdrawal_id: Uuid) -> Result<Option<(u64, i64)>, CustodyError> {
		let row: Option<(Option<i64>, Option<i64>)> = sqlx::query_as("SELECT nonce, expiration FROM withdrawal_broadcasts WHERE withdrawal_id = $1 AND network = 'ton'")
			.bind(withdrawal_id)
			.fetch_optional(&self.pool)
			.await
			.map_err(db_unavailable)?;
		Ok(row.map(|(seqno, valid_until)| (seqno.unwrap_or(0).max(0) as u64, valid_until.unwrap_or(0))))
	}

	/// Replace a stuck send's stored bytes with a freshly signed one at the SAME seqno and a
	/// new validity window. Safe by the wallet's replay rule — only one message per seqno can
	/// ever be accepted, and the message being replaced is provably expired.
	async fn replace_tx(&self, withdrawal_id: Uuid, seqno: u64, valid_until: u32, raw_tx: &str, tx_hash: &str) -> Result<(), CustodyError> {
		sqlx::query("UPDATE withdrawal_broadcasts SET nonce = $2, expiration = $3, raw_tx = $4, tx_hash = $5 WHERE withdrawal_id = $1 AND network = 'ton'")
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

	/// Forget a stored send whose FIRST broadcast the node rejected synchronously — it never
	/// entered the network, so its seqno slot must be freed for the next withdrawal. Never
	/// called for bytes that may have been accepted.
	async fn discard_tx(&self, withdrawal_id: Uuid) -> Result<(), CustodyError> {
		sqlx::query("DELETE FROM withdrawal_broadcasts WHERE withdrawal_id = $1 AND network = 'ton'")
			.bind(withdrawal_id)
			.execute(&self.pool)
			.await
			.map_err(db_unavailable)?;
		Ok(())
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

	/// On-chain Read-First before signing: the treasury's USDT jetton wallet must hold the
	/// amount to send AND the treasury must hold the native TON to attach to the message.
	/// The ledger's rail-liquidity check reads TigerBeetle — accounting, not the hot
	/// wallet's real balances — so this is the real-funds gate. A shortfall parks
	/// (`Rejected`): retrying would wedge the single-worker drain behind an underfunded rail.
	async fn ensure_treasury_funded(&self, treasury: &str, amount: u128) -> Result<(), CustodyError> {
		let usdt = self.rpc.jetton_wallet(treasury, &self.usdt_master).await.map_err(read_err)?.map(|w| w.balance).unwrap_or(0);
		if usdt < amount {
			return Err(CustodyError::Rejected(format!("ton treasury underfunded on-chain: {usdt} USDT units < {amount} needed")));
		}
		let ton = self.rpc.balance(treasury).await.map_err(read_err)?;
		let gas = u128::from(self.msg_value);
		if ton < gas {
			return Err(CustodyError::Rejected(format!("ton treasury gas-underfunded on-chain: {ton} nanoton < {gas} needed")));
		}
		Ok(())
	}

	/// Re-sign a stuck send at its SAME seqno with a fresh validity window and re-broadcast,
	/// unfreezing the strictly-sequential pipeline when a signed message expired before its
	/// seqno turn came (so re-sending the stored — now expired — bytes can never land). Only
	/// acts once the stored message is provably expired; returns `false` if there is nothing
	/// stored or it is still live (the watcher then re-broadcasts the original bytes). Safe by
	/// the wallet's replay rule: at most one message per seqno is ever accepted, and the one
	/// being replaced is expired, so no double-send is possible.
	pub async fn resign_stuck(&self, request: &BroadcastRequest) -> Result<bool, CustodyError> {
		let Some((seqno, valid_until)) = self.stored_seqno(request.withdrawal_id).await? else {
			return Ok(false);
		};
		if now_unix() < valid_until.max(0) as u64 {
			return Ok(false);
		}
		let treasury = self.treasury_address().await?;
		let treasury_jetton_wallet = self.treasury_jetton_wallet(&treasury).await?;
		let amount = request
			.amount
			.to_onchain(Network::Ton)
			.map_err(|e| CustodyError::Rejected(format!("amount not representable on TON: {e}")))?;
		self.ensure_treasury_funded(&treasury, amount).await?;
		let fresh_valid_until = (now_unix() + VALID_WINDOW_SECS) as u32;
		let (boc, msg_hash) = self.sign(request, &treasury, &treasury_jetton_wallet, amount, seqno, fresh_valid_until).await?;
		self.replace_tx(request.withdrawal_id, seqno, fresh_valid_until, &boc, &msg_hash).await?;
		warn!(withdrawal_id = %request.withdrawal_id, seqno, "ton custody: re-signed a stuck expired send at the same seqno — pipeline unfrozen");
		self.submit(&boc, false, request.withdrawal_id).await?;
		Ok(true)
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
	async fn submit(&self, boc: &str, rebroadcast: bool, withdrawal_id: Uuid) -> Result<(), CustodyError> {
		match self.rpc.send_message(boc).await {
			Ok(()) => {
				info!(%withdrawal_id, rebroadcast, "ton custody: broadcast withdrawal message");
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

impl Gateway for TonCustody {}

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
			return self.submit(&boc, true, request.withdrawal_id).await;
		}

		let treasury = self.treasury_address().await?;
		let treasury_jetton_wallet = self.treasury_jetton_wallet(&treasury).await?;
		let amount = request
			.amount
			.to_onchain(Network::Ton)
			.map_err(|e| CustodyError::Rejected(format!("amount not representable on TON: {e}")))?;
		self.ensure_treasury_funded(&treasury, amount).await?;
		let seqno = self.next_seqno(&treasury).await?;
		let valid_until = (now_unix() + VALID_WINDOW_SECS) as u32;
		let (boc, msg_hash) = self.sign(request, &treasury, &treasury_jetton_wallet, amount, seqno, valid_until).await?;

		// Persist BEFORE broadcasting — a crash after this re-broadcasts THIS message (same
		// seqno), never a freshly-signed one.
		self.store_tx(request.withdrawal_id, seqno, valid_until, &boc, &msg_hash).await?;
		match self.submit(&boc, false, request.withdrawal_id).await {
			// The node refused this FIRST send synchronously — the message never entered the
			// network, so free its stored seqno (mirrors BSC's `discard_tx`): a lingering row
			// feeds `next_seqno`'s MAX(nonce) and would wedge every later send above a slot
			// nothing will ever fill. Re-broadcasts are NOT discarded — a rejection there can
			// mean "already accepted".
			Err(CustodyError::Rejected(msg)) => {
				self.discard_tx(request.withdrawal_id).await?;
				Err(CustodyError::Rejected(msg))
			}
			other => other,
		}
	}

	async fn treasury_liquidity(&self, _network: Network) -> Result<Option<Usdt>, CustodyError> {
		let treasury = self.treasury_address().await?;
		// A treasury that never received USDT has no jetton wallet yet — that is a zero
		// balance for the dispatch gate, not an error (broadcast-time provisioning parks it).
		let raw = self.rpc.jetton_wallet(&treasury, &self.usdt_master).await.map_err(read_err)?.map(|w| w.balance).unwrap_or(0);
		let usdt = Usdt::from_onchain(Network::Ton, raw).map_err(|e| CustodyError::Unavailable(format!("ton treasury balance not representable: {e}")))?;
		Ok(Some(usdt))
	}

	async fn treasury_funding(&self, network: Network) -> Result<Option<TreasuryFunding>, CustodyError> {
		let address = self.treasury_address().await?;
		// `treasury_liquidity` already renders a missing jetton wallet as ZERO (an
		// unfunded treasury, not an error); a failed read degrades to None. TON is 9-dp.
		let onchain_usdt = self.treasury_liquidity(network).await.ok().flatten();
		let onchain_gas = self.rpc.balance(&address).await.ok().map(|nanoton| format_native_units(nanoton, 9));
		Ok(Some(TreasuryFunding { address, onchain_usdt, onchain_gas }))
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
