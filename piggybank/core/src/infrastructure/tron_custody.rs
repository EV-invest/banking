//! Tron custody adapter — the relay's "broadcast this withdrawal" seam for the TRC20 rail.
//!
//! The Tron analogue of [`ChainCustody`](super::custody::ChainCustody), with one load-bearing
//! difference: **Tron has no nonce.** EVM's "re-sign is byte-identical forever at the mined nonce"
//! property does not hold — a fresh sign uses a new ref-block + timestamp, hence a new txID. So the
//! crash-safe / no-double-spend rule is:
//!   1. Persist the signed transaction (`withdrawal_broadcasts`, keyed by `withdrawal_id`, with its
//!      `expiration`) BEFORE broadcasting. A relay re-delivery within the window re-broadcasts the
//!      SAME bytes (`DUP_TRANSACTION_ERROR` = idempotent success) — never a second tx.
//!   2. Only after the stored tx has provably **expired without landing** — the SOLIDIFIED head's
//!      timestamp is past `expiration` (+ margin), so no block can ever include it again, AND
//!      `gettransactioninfobyid` finds nothing — is it safe to re-sign at a fresh ref-block and
//!      replace the row. A wall-clock-only expiry check would re-sign while the original could
//!      still land (or already sits in an unreported block) — a double payout.
//!
//! Scope: TRC20 only. The on-chain SETTLE is the [`tron_withdrawal_watcher`](super::tron_withdrawal_watcher)
//! (or an operator's `SettleWithdrawal`); this adapter only gets the bytes onto the chain.

use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use domain::{architecture::Gateway, money::Network};
use evbanking_auth::ServiceTokenSource;
use evbanking_contracts::signer::v1::{ProvisionAddressRequest, SignTrc20TransferRequest, signer_service_client::SignerServiceClient};
use sqlx::PgPool;
use tokio::sync::OnceCell;
use tonic::{Request, transport::Channel};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
	config::TronConfig,
	infrastructure::tron_rpc::{RefBlockParams, TronRpc, TronRpcError},
	ports::custody::{BroadcastRequest, Custody, CustodyError},
};

/// Extra solidified-time that must pass beyond a stored tx's `expiration` before it is
/// treated as provably dead (re-signable). Covers block-slot granularity and node clock
/// tolerance; generous, because the cost of waiting is a minute of latency while the cost
/// of a premature re-sign is a double payout.
const EXPIRATION_MARGIN_MS: i64 = 60_000;

pub struct TronCustody {
	pool: PgPool,
	rpc: TronRpc,
	signer: SignerServiceClient<Channel>,
	service_token: Option<ServiceTokenSource>,
	usdt_contract: String,
	fee_limit: i64,
	treasury_address: OnceCell<String>,
}
impl TronCustody {
	pub fn new(pool: PgPool, channel: Channel, service_token: Option<ServiceTokenSource>, config: &TronConfig) -> Self {
		Self {
			pool,
			rpc: TronRpc::new(config.rpc_url.clone(), config.api_key.clone(), config.expiration_secs),
			signer: SignerServiceClient::new(channel),
			service_token,
			usdt_contract: config.usdt_contract.clone(),
			fee_limit: config.fee_limit,
			treasury_address: OnceCell::new(),
		}
	}

	/// The treasury's Tron address, resolved once via `ProvisionAddress` (the reserved nil user id)
	/// and cached — so the composition root can resolve + log it at boot. The operator funds this
	/// address out-of-band (USDT for liquidity, TRX for fees) before withdrawals can settle.
	pub async fn treasury_address(&self) -> Result<String, CustodyError> {
		self.treasury_address
			.get_or_try_init(|| async {
				let mut request = Request::new(ProvisionAddressRequest {
					user_id: Uuid::nil().to_string(),
					network: "trc20".to_owned(),
				});
				if let Some(token) = &self.service_token {
					request = token.authorize(request);
				}
				let response = self
					.signer
					.clone()
					.provision_address(request)
					.await
					.map_err(|s| CustodyError::Unavailable(format!("resolve tron treasury address: {}", s.message())))?
					.into_inner();
				if response.address_kind != "derived" {
					return Err(CustodyError::Rejected(format!("tron treasury address is not fundable (kind={})", response.address_kind)));
				}
				info!(treasury = %response.address, "tron custody: treasury hot wallet — fund it with USDT (liquidity) + TRX (fees)");
				Ok(response.address)
			})
			.await
			.cloned()
	}

	async fn stored_tx(&self, withdrawal_id: Uuid) -> Result<Option<StoredTx>, CustodyError> {
		let row: Option<(String, String, Option<i64>)> = sqlx::query_as("SELECT raw_tx, tx_hash, expiration FROM withdrawal_broadcasts WHERE withdrawal_id = $1 AND network = 'trc20'")
			.bind(withdrawal_id)
			.fetch_optional(&self.pool)
			.await
			.map_err(db_unavailable)?;
		Ok(row.map(|(raw_tx, txid, expiration)| StoredTx {
			raw_tx,
			txid,
			expiration: expiration.unwrap_or(0),
		}))
	}

	async fn store_tx(&self, withdrawal_id: Uuid, signed: &Signed) -> Result<(), CustodyError> {
		sqlx::query(
			"INSERT INTO withdrawal_broadcasts (withdrawal_id, network, nonce, raw_tx, tx_hash, expiration) VALUES ($1, 'trc20', NULL, $2, $3, $4) ON CONFLICT (withdrawal_id) DO NOTHING",
		)
		.bind(withdrawal_id)
		.bind(&signed.raw_tx)
		.bind(&signed.txid)
		.bind(signed.expiration)
		.execute(&self.pool)
		.await
		.map_err(db_unavailable)?;
		Ok(())
	}

	/// Replace a provably-dead (expired, never mined) broadcast with a freshly signed one. Only
	/// ever reached after the on-chain check confirms the old txid is nowhere, so this can't
	/// overwrite a live transaction.
	async fn replace_tx(&self, withdrawal_id: Uuid, signed: &Signed) -> Result<(), CustodyError> {
		sqlx::query("UPDATE withdrawal_broadcasts SET raw_tx = $2, tx_hash = $3, expiration = $4 WHERE withdrawal_id = $1")
			.bind(withdrawal_id)
			.bind(&signed.raw_tx)
			.bind(&signed.txid)
			.bind(signed.expiration)
			.execute(&self.pool)
			.await
			.map_err(db_unavailable)?;
		Ok(())
	}

	/// Sign the withdrawal's USDT transfer from the treasury key via the signer (empty
	/// `from_user_id` ⇒ the signer resolves the treasury key itself).
	async fn sign(&self, request: &BroadcastRequest, amount_base_units: u128, refs: &RefBlockParams) -> Result<Signed, CustodyError> {
		let mut signer_request = Request::new(SignTrc20TransferRequest {
			from_user_id: String::new(), // empty ⇒ treasury hot wallet
			network: "trc20".to_owned(),
			token_contract: self.usdt_contract.clone(),
			to_address: request.address.as_str().to_owned(),
			amount: amount_base_units.to_string(),
			ref_block_bytes: refs.ref_block_bytes.clone(),
			ref_block_hash: refs.ref_block_hash.clone(),
			expiration: refs.expiration,
			timestamp: refs.timestamp,
			fee_limit: self.fee_limit,
		});
		if let Some(token) = &self.service_token {
			signer_request = token.authorize(signer_request);
		}
		let response = self.signer.clone().sign_trc20_transfer(signer_request).await.map_err(|s| match s.code() {
			tonic::Code::Unavailable | tonic::Code::DeadlineExceeded => CustodyError::Unavailable(format!("signer: {}", s.message())),
			_ => CustodyError::Rejected(format!("signer: {}", s.message())),
		})?;
		let response = response.into_inner();
		Ok(Signed {
			raw_tx: response.signed_tx,
			txid: response.txid,
			expiration: response.expiration,
		})
	}

	/// On-chain Read-First before signing: the treasury must hold the USDT to send AND
	/// TRX up to the fee limit. The ledger's rail-liquidity check reads TigerBeetle —
	/// accounting, not the hot wallet's real balances — and a node-level refusal at
	/// broadcast is noisier and later than refusing here. A shortfall parks (`Rejected`):
	/// retrying would wedge the single-worker drain behind an underfunded rail.
	async fn ensure_treasury_funded(&self, request: &BroadcastRequest) -> Result<(), CustodyError> {
		let treasury = self.treasury_address().await?;
		let state = self.rpc.account_state(&treasury).await.map_err(read_err)?;
		let needed = request
			.amount
			.to_onchain(Network::Trc20)
			.map_err(|e| CustodyError::Rejected(format!("amount not representable on trc20: {e}")))?;
		let usdt = state.trc20(&self.usdt_contract);
		if usdt < needed {
			return Err(CustodyError::Rejected(format!("tron treasury underfunded on-chain: {usdt} USDT units < {needed} needed")));
		}
		if state.trx < self.fee_limit as u128 {
			return Err(CustodyError::Rejected(format!(
				"tron treasury fee-underfunded on-chain: {} SUN < fee limit {}",
				state.trx, self.fee_limit
			)));
		}
		Ok(())
	}

	/// Sign a fresh transaction (fetching a recent ref-block) for the withdrawal's net amount.
	async fn sign_fresh(&self, request: &BroadcastRequest) -> Result<Signed, CustodyError> {
		let amount = request
			.amount
			.to_onchain(Network::Trc20)
			.map_err(|e| CustodyError::Rejected(format!("amount not representable on trc20: {e}")))?;
		let refs = self.rpc.ref_block_params().await.map_err(read_err)?;
		self.sign(request, amount, &refs).await
	}

	/// Re-handle a withdrawal that already has a stored transaction. Within the validity window,
	/// re-broadcast the SAME bytes (idempotent). Past expiration, only re-sign once the stored
	/// txid is **provably** dead — otherwise treat it as landed (or landable) and let the
	/// confirmation watcher settle it.
	///
	/// "Provably dead" is a chain fact, not a wall-clock one: the local clock passing
	/// `expiration` only means the tx *looks* expired — it may sit in a block the node hasn't
	/// reported yet, or in one still below solidity that a re-signed twin would then double-pay.
	/// Tron includes a tx only in blocks whose timestamp ≤ its `expiration` and block timestamps
	/// are slot-monotonic, so once the SOLIDIFIED head's timestamp is past `expiration` (plus a
	/// margin for slot/clock tolerance) no future block can carry it, and any block that already
	/// did is itself solidified — a missing receipt is then final, never merely late.
	async fn resubmit(&self, request: &BroadcastRequest, stored: StoredTx) -> Result<(), CustodyError> {
		if now_ms() < stored.expiration {
			return self.submit(&stored.raw_tx, true).await;
		}
		let solid_ts = self.rpc.solid_block_timestamp().await.map_err(read_err)?;
		if solid_ts <= stored.expiration.saturating_add(EXPIRATION_MARGIN_MS) {
			// Looks expired but not provably dead yet — a transient wait (~a minute of
			// solidification), NOT a park and NOT a re-sign. The relay retries this seq.
			return Err(CustodyError::Unavailable(format!(
				"stored tx {} past expiration but not yet provably dead (solid head ts {solid_ts} <= expiration {} + margin) — waiting for solidification",
				stored.txid, stored.expiration
			)));
		}
		match self.rpc.transaction_info(&stored.txid).await.map_err(read_err)? {
			Some(_) => {
				info!(withdrawal_id = %request.withdrawal_id, txid = %stored.txid, "tron custody: stored transaction already on-chain — idempotent");
				Ok(())
			}
			None => {
				warn!(withdrawal_id = %request.withdrawal_id, txid = %stored.txid, "tron custody: stored transaction provably dead (solidified past expiration, no receipt) — re-signing at a fresh ref-block");
				self.ensure_treasury_funded(request).await?;
				let signed = self.sign_fresh(request).await?;
				self.replace_tx(request.withdrawal_id, &signed).await?;
				self.submit(&signed.raw_tx, false).await
			}
		}
	}

	/// Submit a signed transaction. `rebroadcast` marks a re-send of stored bytes, where a
	/// duplicate is idempotent success rather than a fresh-send failure.
	async fn submit(&self, signed_tx_hex: &str, rebroadcast: bool) -> Result<(), CustodyError> {
		match self.rpc.broadcast_hex(signed_tx_hex).await {
			Ok(txid) => {
				info!(%txid, rebroadcast, "tron custody: broadcast withdrawal transaction");
				Ok(())
			}
			// Nothing reached the chain — retry from the same stored tx.
			Err(TronRpcError::Transport(detail)) => Err(CustodyError::Unavailable(detail)),
			// Already submitted — re-sending the same signed bytes is a no-op success.
			Err(TronRpcError::Rpc(msg)) if is_duplicate(&msg) => {
				info!(reason = %msg, "tron custody: transaction already submitted — idempotent");
				Ok(())
			}
			// A genuine rejection (validation, expiration on a fresh send, out of energy) — park.
			Err(TronRpcError::Rpc(msg)) => {
				warn!(reason = %msg, "tron custody: node rejected the transaction — parking");
				Err(CustodyError::Rejected(msg))
			}
		}
	}
}

/// A previously signed+stored Tron transaction for a withdrawal.
struct StoredTx {
	raw_tx: String,
	txid: String,
	expiration: i64,
}

impl Gateway for TronCustody {}

#[async_trait]
impl Custody for TronCustody {
	async fn broadcast(&self, request: &BroadcastRequest) -> Result<(), CustodyError> {
		debug_assert!(
			matches!(request.network, Network::Trc20),
			"TronCustody is the TRC20 adapter; the registry must not route {} here",
			request.network
		);
		// Idempotent: re-send the stored bytes (or re-sign only if provably expired+unmined).
		if let Some(stored) = self.stored_tx(request.withdrawal_id).await? {
			return self.resubmit(request, stored).await;
		}
		self.ensure_treasury_funded(request).await?;
		let signed = self.sign_fresh(request).await?;
		// Persist BEFORE broadcasting — a crash after this re-broadcasts THIS tx (same bytes/txid),
		// never a freshly-signed one with a different txid.
		self.store_tx(request.withdrawal_id, &signed).await?;
		self.submit(&signed.raw_tx, false).await
	}
}

/// A signed Tron transaction returned by the signer.
struct Signed {
	raw_tx: String,
	txid: String,
	expiration: i64,
}

/// A read-path RPC failure (ref-block, receipt) is always retryable — nothing was sent.
fn read_err(err: TronRpcError) -> CustodyError {
	CustodyError::Unavailable(err.to_string())
}

fn db_unavailable(err: sqlx::Error) -> CustodyError {
	CustodyError::Unavailable(format!("tron custody db: {err}"))
}

/// Node responses meaning the transaction is already submitted (idempotent re-send).
fn is_duplicate(msg: &str) -> bool {
	let m = msg.to_uppercase();
	m.contains("DUP_TRANSACTION") || m.contains("ALREADY EXISTS") || m.contains("ALREADY KNOWN")
}

fn now_ms() -> i64 {
	SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
	use super::is_duplicate;

	#[test]
	fn recognises_duplicate_broadcast_responses() {
		assert!(is_duplicate("DUP_TRANSACTION_ERROR: dup transaction"));
		assert!(is_duplicate("transaction already exists"));
		assert!(!is_duplicate("TRANSACTION_EXPIRATION_ERROR: expired"));
		assert!(!is_duplicate("CONTRACT_VALIDATE_ERROR: balance is not sufficient"));
	}
}
