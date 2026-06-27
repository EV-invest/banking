//! Custody adapters — the relay's "broadcast this withdrawal" seam.
//!
//! [`StubCustody`] is the no-op stand-in (an operator settles manually). [`ChainCustody`]
//! is the real one: it signs the withdrawal's ERC-20 transfer via the signer (the key never
//! leaves there) and broadcasts it to BSC. **Crash-safety / no double-spend:** the signed
//! transaction is persisted (`withdrawal_broadcasts`, keyed by `withdrawal_id`) BEFORE it is
//! sent, so an at-least-once relay re-delivery re-broadcasts the SAME bytes (same nonce)
//! rather than signing a new one — a withdrawal can never go out twice under two nonces.
//!
//! Scope: BEP20 only. The on-chain SETTLE (reducing the ledger's rail custody) is a separate
//! step — an operator's `SettleWithdrawal` (or a future confirmation watcher) on the mined
//! transaction; this adapter only gets the bytes onto the chain.

use async_trait::async_trait;
use domain::money::Network;
use evbanking_auth::ServiceTokenSource;
use evbanking_contracts::signer::v1::{ProvisionAddressRequest, SignErc20TransferRequest, signer_service_client::SignerServiceClient};
use sqlx::PgPool;
use tokio::sync::OnceCell;
use tonic::{Request, transport::Channel};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
	infrastructure::bsc_rpc::{BscRpc, RpcError},
	ports::custody::{BroadcastRequest, Custody, CustodyError},
};

/// No-op custody: logs and returns success. An operator supplies the real on-chain tx ref
/// later via `BalanceService.SettleWithdrawal`. Used when BSC is unconfigured.
pub struct StubCustody;

#[async_trait]
impl Custody for StubCustody {
	async fn broadcast(&self, request: &BroadcastRequest) -> Result<(), CustodyError> {
		info!(
			withdrawal_id = %request.withdrawal_id,
			network = %request.network,
			address = request.address.as_str(),
			amount = %request.amount,
			"stub custody: pretending to broadcast a withdrawal (no real chain); awaiting operator settle/fail"
		);
		Ok(())
	}
}

/// Real BSC custody: sign via the signer's treasury key, broadcast via the node.
pub struct ChainCustody {
	pool: PgPool,
	rpc: BscRpc,
	signer: SignerServiceClient<Channel>,
	service_token: Option<ServiceTokenSource>,
	chain_id: u64,
	usdt_contract: String,
	gas_limit: u64,
	/// The treasury hot wallet's address (the withdrawal source), resolved once via the
	/// signer and cached. Funds — USDT to send, BNB for gas — are deposited here out-of-band.
	treasury_address: OnceCell<String>,
}

impl ChainCustody {
	pub fn new(pool: PgPool, rpc: BscRpc, signer: SignerServiceClient<Channel>, service_token: Option<ServiceTokenSource>, chain_id: u64, usdt_contract: String, gas_limit: u64) -> Self {
		Self {
			pool,
			rpc,
			signer,
			service_token,
			chain_id,
			usdt_contract,
			gas_limit,
			treasury_address: OnceCell::new(),
		}
	}

	/// The treasury's BEP20 address, resolved once via `ProvisionAddress` (the reserved nil
	/// user id) and cached. A transient failure leaves the cell empty so a later call retries.
	/// Public so the composition root can resolve + log it at boot — the operator funds this
	/// address out-of-band (USDT for liquidity, BNB for gas) before withdrawals can settle.
	pub async fn treasury_address(&self) -> Result<String, CustodyError> {
		self.treasury_address
			.get_or_try_init(|| async {
				let mut request = Request::new(ProvisionAddressRequest {
					user_id: Uuid::nil().to_string(),
					network: "bep20".to_owned(),
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
				info!(treasury = %response.address, "chain custody: treasury hot wallet — fund it with USDT (liquidity) + BNB (gas)");
				Ok(response.address)
			})
			.await
			.cloned()
	}

	/// The previously signed+stored raw transaction for this withdrawal, if any.
	async fn stored_tx(&self, withdrawal_id: Uuid) -> Result<Option<String>, CustodyError> {
		sqlx::query_scalar::<_, String>("SELECT raw_tx FROM withdrawal_broadcasts WHERE withdrawal_id = $1")
			.bind(withdrawal_id)
			.fetch_optional(&self.pool)
			.await
			.map_err(db_unavailable)
	}

	async fn store_tx(&self, withdrawal_id: Uuid, nonce: u64, raw_tx: &str, tx_hash: &str) -> Result<(), CustodyError> {
		sqlx::query("INSERT INTO withdrawal_broadcasts (withdrawal_id, nonce, raw_tx, tx_hash) VALUES ($1, $2, $3, $4) ON CONFLICT (withdrawal_id) DO NOTHING")
			.bind(withdrawal_id)
			.bind(nonce as i64)
			.bind(raw_tx)
			.bind(tx_hash)
			.execute(&self.pool)
			.await
			.map_err(db_unavailable)?;
		Ok(())
	}

	/// The next nonce for the treasury: the max of the chain's pending count and one past the
	/// highest nonce we've already assigned — monotonic even if a public node lags, and it
	/// catches up to the chain after a restart.
	async fn next_nonce(&self, treasury: &str) -> Result<u64, CustodyError> {
		let chain = self.rpc.pending_nonce(treasury).await.map_err(read_err)?;
		let local_max: Option<i64> = sqlx::query_scalar("SELECT MAX(nonce) FROM withdrawal_broadcasts")
			.fetch_one(&self.pool)
			.await
			.map_err(db_unavailable)?;
		let local_next = local_max.map(|n| n as u64 + 1).unwrap_or(0);
		Ok(chain.max(local_next))
	}

	/// Sign the withdrawal's USDT transfer from the treasury key via the signer (the signer
	/// resolves the treasury key itself from the empty `from_user_id`).
	async fn sign(&self, request: &BroadcastRequest, nonce: u64, gas_price: u128) -> Result<(String, String), CustodyError> {
		let mut signer_request = Request::new(SignErc20TransferRequest {
			from_user_id: String::new(), // empty ⇒ treasury hot wallet
			network: "bep20".to_owned(),
			token_contract: self.usdt_contract.clone(),
			to_address: request.address.as_str().to_owned(),
			amount: request.amount.base_units().to_string(),
			chain_id: self.chain_id,
			nonce,
			gas_price: gas_price.to_string(),
			gas_limit: self.gas_limit,
		});
		if let Some(token) = &self.service_token {
			signer_request = token.authorize(signer_request);
		}
		let response = self.signer.clone().sign_erc20_transfer(signer_request).await.map_err(|s| match s.code() {
			tonic::Code::Unavailable | tonic::Code::DeadlineExceeded => CustodyError::Unavailable(format!("signer: {}", s.message())),
			_ => CustodyError::Rejected(format!("signer: {}", s.message())),
		})?;
		let response = response.into_inner();
		Ok((response.raw_tx, response.tx_hash))
	}

	/// Submit a raw transaction. `rebroadcast` marks a re-send of a previously stored tx,
	/// where "nonce too low" means our tx already mined (idempotent success) rather than a
	/// stale-nonce failure on a fresh send.
	async fn submit(&self, raw_tx: &str, rebroadcast: bool) -> Result<(), CustodyError> {
		match self.rpc.send_raw_transaction(raw_tx).await {
			Ok(tx_hash) => {
				info!(%tx_hash, rebroadcast, "chain custody: broadcast withdrawal transaction");
				Ok(())
			}
			// Nothing reached the chain — retry from the same stored tx.
			Err(RpcError::Transport(detail)) => Err(CustodyError::Unavailable(detail)),
			// Already in the mempool — sending the same signed bytes again is a no-op success.
			Err(RpcError::Rpc(msg)) if already_accepted(&msg) => {
				info!(reason = %msg, "chain custody: transaction already submitted — idempotent");
				Ok(())
			}
			// A re-broadcast whose tx already mined reports "nonce too low" — success.
			Err(RpcError::Rpc(msg)) if rebroadcast && msg.to_lowercase().contains("nonce too low") => {
				info!(reason = %msg, "chain custody: stored transaction already mined — idempotent");
				Ok(())
			}
			// A genuine rejection (bad tx, insufficient funds/gas) — park for intervention.
			Err(RpcError::Rpc(msg)) => {
				warn!(reason = %msg, "chain custody: node rejected the transaction — parking");
				Err(CustodyError::Rejected(msg))
			}
		}
	}
}

#[async_trait]
impl Custody for ChainCustody {
	async fn broadcast(&self, request: &BroadcastRequest) -> Result<(), CustodyError> {
		if !matches!(request.network, Network::Bep20) {
			return Err(CustodyError::Rejected(format!("on-chain withdrawal is not wired for {} yet", request.network)));
		}
		// Idempotent: if we already signed+stored a transaction for this withdrawal, re-send
		// THOSE exact bytes rather than signing a new one (no second nonce can ever go out).
		if let Some(raw_tx) = self.stored_tx(request.withdrawal_id).await? {
			return self.submit(&raw_tx, true).await;
		}

		let treasury = self.treasury_address().await?;
		let nonce = self.next_nonce(&treasury).await?;
		let gas_price = self.rpc.gas_price().await.map_err(read_err)?;
		let (raw_tx, tx_hash) = self.sign(request, nonce, gas_price).await?;

		// Persist BEFORE broadcasting — a crash after this re-broadcasts THIS tx (same nonce),
		// never a freshly-signed one with a different nonce.
		self.store_tx(request.withdrawal_id, nonce, &raw_tx, &tx_hash).await?;
		self.submit(&raw_tx, false).await
	}
}

/// A read-path RPC failure (nonce/gas) is always retryable — nothing was sent.
fn read_err(err: RpcError) -> CustodyError {
	CustodyError::Unavailable(err.to_string())
}

fn db_unavailable(err: sqlx::Error) -> CustodyError {
	CustodyError::Unavailable(format!("custody db: {err}"))
}

/// Node responses that mean the transaction is already submitted (idempotent re-send).
fn already_accepted(msg: &str) -> bool {
	let m = msg.to_lowercase();
	m.contains("already known") || m.contains("known transaction") || m.contains("already imported") || m.contains("already exists")
}

#[cfg(test)]
mod tests {
	use super::already_accepted;

	#[test]
	fn recognises_already_submitted_responses() {
		assert!(already_accepted("already known"));
		assert!(already_accepted("ALREADY KNOWN"));
		assert!(already_accepted("known transaction: 0xabc"));
		assert!(already_accepted("transaction already imported"));
		assert!(!already_accepted("insufficient funds for gas * price + value"));
		assert!(!already_accepted("nonce too low"));
	}
}
