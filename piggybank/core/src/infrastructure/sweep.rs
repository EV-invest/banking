//! Treasury sweep — consolidates user deposit balances into the treasury hot wallet.
//!
//! A deposit credits the user's ledger claim ([`deposit_watcher`](super::deposit_watcher)),
//! but the USDT physically lands on the user's *derived deposit address*, not in the
//! treasury that pays withdrawals. This background job moves it: for each deposit address
//! holding USDT, it signs an ERC-20 transfer **from that address** (the signer holds the
//! key) to the treasury and broadcasts it — so deposits become treasury liquidity.
//!
//! **Gas.** A user address holds only USDT, and an ERC-20 transfer costs BNB. So a separate
//! **gas station** account (its own reserved id ⇒ its own nonce sequence, independent of the
//! treasury's — the withdrawal path is never raced for a nonce) tops the address up with a
//! little BNB first; the next cycle, with gas present, sweeps the USDT.
//!
//! **Idempotency (no double-send), with no extra persistence.** Three on-chain facts carry it:
//!   - the USDT sweep is signed at the address's **mined** (`latest`) nonce, so a re-sign of
//!     an unconfirmed sweep is the *same* deterministic transaction (RFC-6979) — an
//!     idempotent "already known" re-broadcast, never a second tx at the next nonce;
//!   - the on-chain **balance** is the truth — once a sweep mines, the address reads zero and
//!     drops out, so nothing re-sweeps;
//!   - the gas station's nonce is an in-memory monotonic counter (seeded from the chain), so
//!     several top-ups in one cycle don't collide, plus a short in-memory grace so we don't
//!     pile up top-ups to one address while the first confirms.
//!
//! Read-mostly and **opt-in** (`SWEEP_ENABLED`): it never touches TigerBeetle (the deposit
//! was already credited; this only relocates the on-chain custody), and it is off unless the
//! operator funds the gas station and turns it on. Scope: **BEP20 only**. A stuck
//! underpriced transaction (no replacement-by-fee here) blocking a nonce is a known
//! operational residual — manual intervention, like the reaper's stuck-withdrawal alert.

use std::{
	collections::HashMap,
	sync::Mutex,
	time::{Duration, Instant},
};

use evbanking_auth::ServiceTokenSource;
use evbanking_contracts::signer::v1::{ProvisionAddressRequest, SignErc20TransferRequest, SignNativeTransferRequest, signer_service_client::SignerServiceClient};
use sqlx::PgPool;
use tokio::sync::OnceCell;
use tokio_util::sync::CancellationToken;
use tonic::{Request, transport::Channel};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{
	config::{BscConfig, SweepConfig},
	infrastructure::bsc_rpc::{BscRpc, RpcError},
};

/// The reserved gas-station account id, distinct from the nil treasury: a wallet holding
/// only BNB, used to top up user deposit addresses with gas. A separate account means a
/// separate nonce sequence, so the sweep never races the withdrawal custody path.
const GAS_STATION: Uuid = Uuid::from_u128(1);

/// 1 gwei in wei. Gas prices are rounded UP to a whole gwei so minor node-to-node wobble
/// doesn't change a re-signed transaction's bytes (keeping the idempotent-re-sign property).
const GWEI: u128 = 1_000_000_000;

/// Gas for a plain native (BNB) value transfer.
const NATIVE_TRANSFER_GAS: u64 = 21_000;

pub struct Sweep {
	pool: PgPool,
	rpc: BscRpc,
	signer: SignerServiceClient<Channel>,
	service_token: Option<ServiceTokenSource>,
	usdt_contract: String,
	chain_id: u64,
	transfer_gas_limit: u64,
	config: SweepConfig,
	treasury: OnceCell<String>,
	gas_station: OnceCell<String>,
	state: Mutex<GasState>,
}
impl Sweep {
	pub fn new(pool: PgPool, channel: Channel, service_token: Option<ServiceTokenSource>, bsc: &BscConfig, config: SweepConfig) -> Self {
		Self {
			pool,
			rpc: BscRpc::new(bsc.rpc_url.clone()),
			signer: SignerServiceClient::new(channel),
			service_token,
			usdt_contract: bsc.usdt_contract.clone(),
			chain_id: bsc.chain_id,
			transfer_gas_limit: bsc.gas_limit,
			config,
			treasury: OnceCell::new(),
			gas_station: OnceCell::new(),
			state: Mutex::new(GasState::default()),
		}
	}

	pub async fn run(self, shutdown: CancellationToken) {
		info!(
			min_usdt = self.config.min_usdt,
			poll_secs = self.config.poll_secs,
			"sweep: consolidating BEP20 deposits into the treasury"
		);
		// Resolve + log the system wallets up front so the operator can fund the gas station
		// with BNB (it pays the gas to move user USDT). Best-effort — retried each cycle.
		match (self.address(&self.treasury, Uuid::nil()).await, self.address(&self.gas_station, GAS_STATION).await) {
			(Ok(treasury), Ok(gas_station)) => info!(%treasury, %gas_station, "sweep: fund the gas station with BNB — it pays gas to sweep user USDT into the treasury"),
			_ => warn!("sweep: could not resolve the treasury/gas-station addresses yet (will retry each cycle)"),
		}
		loop {
			if let Err(err) = self.sweep_once().await {
				warn!("sweep: cycle failed, retrying next poll: {err}");
			}
			tokio::select! {
				() = shutdown.cancelled() => {
					info!("sweep: shutdown requested — stopping");
					return;
				}
				() = tokio::time::sleep(Duration::from_secs(self.config.poll_secs)) => {}
			}
		}
	}

	async fn sweep_once(&self) -> Result<(), SweepError> {
		let treasury = self.address(&self.treasury, Uuid::nil()).await?;
		let gas_station = self.address(&self.gas_station, GAS_STATION).await?;
		for (user_id, address) in self.deposit_addresses().await? {
			// Never sweep a system wallet into itself (they aren't normally in this table, but
			// guard anyway — a self-transfer would burn gas for nothing).
			if address.eq_ignore_ascii_case(&treasury) || address.eq_ignore_ascii_case(&gas_station) {
				continue;
			}
			// One bad address (RPC blip, an out-of-gas sender) must not stop the others.
			if let Err(err) = self.sweep_address(user_id, &address, &treasury, &gas_station).await {
				warn!(%address, "sweep: address cycle failed (continuing): {err}");
			}
		}
		Ok(())
	}

	async fn sweep_address(&self, user_id: Uuid, address: &str, treasury: &str, gas_station: &str) -> Result<(), SweepError> {
		let usdt = self.rpc.erc20_balance(&self.usdt_contract, address).await.map_err(read_err)?;
		if usdt < self.config.min_usdt {
			return Ok(());
		}
		let gas_price = self.gas_price().await?;
		let needed = gas_price.saturating_mul(self.transfer_gas_limit as u128);
		let bnb = self.rpc.bnb_balance(address).await.map_err(read_err)?;
		if bnb < needed {
			// Not enough gas to move the USDT — top up from the gas station, sweep next cycle.
			return self.top_up_gas(address, gas_station, needed, gas_price).await;
		}
		// Sign at the MINED nonce, so a re-sign of an unconfirmed sweep is the same tx.
		let nonce = self.rpc.latest_nonce(address).await.map_err(read_err)?;
		let (raw, hash) = self.sign_sweep(user_id, address, treasury, usdt, nonce, gas_price).await?;
		self.broadcast(&raw, &hash, "sweep", address).await;
		Ok(())
	}

	async fn top_up_gas(&self, address: &str, gas_station: &str, needed: u128, gas_price: u128) -> Result<(), SweepError> {
		// Best-effort grace: skip if we topped this address up very recently (still confirming).
		if let Ok(state) = self.state.lock()
			&& let Some(at) = state.recent_topups.get(address)
			&& at.elapsed() < Duration::from_secs(self.config.topup_grace_secs)
		{
			return Ok(());
		}
		let nonce = self.next_gas_nonce(gas_station).await?;
		let drop = needed.saturating_mul(self.config.gas_drop_multiple).max(self.config.min_gas_drop_wei);
		let (raw, hash) = self.sign_native(address, drop, nonce, gas_price).await?;
		// Record before broadcasting so a slow/failed send still dedups the next cycle.
		if let Ok(mut state) = self.state.lock() {
			state.recent_topups.insert(address.to_owned(), Instant::now());
		}
		info!(%address, drop, "sweep: topping up gas for a deposit address");
		self.broadcast(&raw, &hash, "gas", address).await;
		Ok(())
	}

	/// The next gas-station nonce: the chain's pending count, but never below our in-memory
	/// high-water mark, so several top-ups within one cycle (or before the node's pending
	/// count catches up) get distinct, monotonic nonces.
	async fn next_gas_nonce(&self, gas_station: &str) -> Result<u64, SweepError> {
		let chain = self.rpc.pending_nonce(gas_station).await.map_err(read_err)?;
		let mut state = self.state.lock().map_err(|_| SweepError::Config("sweep state mutex poisoned".into()))?;
		let next = state.next_nonce.map_or(chain, |n| n.max(chain));
		state.next_nonce = Some(next + 1);
		Ok(next)
	}

	/// Live gas price, rounded UP to a whole gwei — stable across node wobble so a re-signed
	/// sweep keeps identical bytes (the idempotent-re-broadcast property), and never zero.
	async fn gas_price(&self) -> Result<u128, SweepError> {
		let raw = self.rpc.gas_price().await.map_err(read_err)?;
		Ok(raw.div_ceil(GWEI).max(1) * GWEI)
	}

	async fn sign_sweep(&self, user_id: Uuid, address: &str, treasury: &str, amount: u128, nonce: u64, gas_price: u128) -> Result<(String, String), SweepError> {
		let mut request = Request::new(SignErc20TransferRequest {
			from_user_id: user_id.to_string(), // the deposit address's owner — the signer holds its key
			network: "bep20".to_owned(),
			token_contract: self.usdt_contract.clone(),
			to_address: treasury.to_owned(),
			amount: amount.to_string(),
			chain_id: self.chain_id,
			nonce,
			gas_price: gas_price.to_string(),
			gas_limit: self.transfer_gas_limit,
		});
		if let Some(token) = &self.service_token {
			request = token.authorize(request);
		}
		let response = self
			.signer
			.clone()
			.sign_erc20_transfer(request)
			.await
			.map_err(|s| SweepError::Signer(format!("sweep {address}: {}", s.message())))?
			.into_inner();
		Ok((response.raw_tx, response.tx_hash))
	}

	async fn sign_native(&self, to: &str, amount: u128, nonce: u64, gas_price: u128) -> Result<(String, String), SweepError> {
		let mut request = Request::new(SignNativeTransferRequest {
			from_user_id: GAS_STATION.to_string(),
			network: "bep20".to_owned(),
			to_address: to.to_owned(),
			amount: amount.to_string(),
			chain_id: self.chain_id,
			nonce,
			gas_price: gas_price.to_string(),
			gas_limit: NATIVE_TRANSFER_GAS,
		});
		if let Some(token) = &self.service_token {
			request = token.authorize(request);
		}
		let response = self
			.signer
			.clone()
			.sign_native_transfer(request)
			.await
			.map_err(|s| SweepError::Signer(format!("gas top-up {to}: {}", s.message())))?
			.into_inner();
		Ok((response.raw_tx, response.tx_hash))
	}

	/// Broadcast a signed transaction, classifying the outcome per-address (never fails the
	/// whole cycle): an idempotent re-send is benign, a sender out of funds is a loud alert
	/// (fund the gas station / treasury), a transport blip retries next cycle.
	async fn broadcast(&self, raw_tx: &str, tx_hash: &str, kind: &str, address: &str) {
		match self.rpc.send_raw_transaction(raw_tx).await {
			Ok(hash) => info!(%hash, kind, %address, "sweep: broadcast transaction"),
			Err(RpcError::Transport(detail)) => warn!(kind, %address, "sweep: transport error (retry next cycle): {detail}"),
			Err(RpcError::Rpc(msg)) if is_idempotent(&msg) => info!(kind, %address, reason = %msg, "sweep: transaction already in flight — idempotent"),
			Err(RpcError::Rpc(msg)) if msg.to_lowercase().contains("insufficient funds") =>
				error!(kind, %address, %tx_hash, "sweep: SENDER OUT OF FUNDS — fund the gas station (BNB) / treasury: {msg}"),
			Err(RpcError::Rpc(msg)) => warn!(kind, %address, "sweep: node rejected the transaction: {msg}"),
		}
	}

	/// A system wallet's BEP20 address, resolved once via `ProvisionAddress` (`Uuid::nil()` =
	/// treasury, [`GAS_STATION`] = gas station) and cached. A transient failure leaves the
	/// cell empty so a later cycle retries.
	async fn address(&self, cell: &OnceCell<String>, id: Uuid) -> Result<String, SweepError> {
		cell.get_or_try_init(|| async {
			let mut request = Request::new(ProvisionAddressRequest {
				user_id: id.to_string(),
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
				.map_err(|s| SweepError::Signer(format!("resolve system wallet {id}: {}", s.message())))?
				.into_inner();
			if response.address_kind != "derived" {
				return Err(SweepError::Config(format!("system wallet {id} is not a derived address (kind={})", response.address_kind)));
			}
			Ok(response.address)
		})
		.await
		.cloned()
	}

	async fn deposit_addresses(&self) -> Result<Vec<(Uuid, String)>, SweepError> {
		sqlx::query_as::<_, (Uuid, String)>("SELECT user_id, address FROM user_deposit_addresses WHERE network = 'bep20' AND address_kind = 'derived'")
			.fetch_all(&self.pool)
			.await
			.map_err(|e| SweepError::Db(e.to_string()))
	}
}

#[derive(Default)]
struct GasState {
	/// In-memory monotonic next nonce for the gas station, so several top-ups in one cycle
	/// get distinct nonces even when the node's pending count lags. Seeded/bumped from the
	/// chain, never allowed to go backwards.
	next_nonce: Option<u64>,
	/// Last time a top-up was sent to an address — a best-effort grace so we don't pile up
	/// top-ups while one confirms (in-memory; a restart may cost one extra harmless top-up).
	recent_topups: HashMap<String, Instant>,
}

/// Node responses that mean our transaction is already accounted for — a re-send is a no-op.
/// `nonce too low` ⇒ it already mined; `already known` ⇒ already in the mempool;
/// `replacement transaction underpriced` ⇒ the prior identical-nonce tx still stands.
fn is_idempotent(msg: &str) -> bool {
	let m = msg.to_lowercase();
	m.contains("already known") || m.contains("known transaction") || m.contains("nonce too low") || m.contains("replacement transaction underpriced") || m.contains("already imported")
}

fn read_err(err: RpcError) -> SweepError {
	SweepError::Rpc(err.to_string())
}

#[derive(Debug, thiserror::Error)]
enum SweepError {
	#[error("rpc: {0}")]
	Rpc(String),
	#[error("signer: {0}")]
	Signer(String),
	#[error("db: {0}")]
	Db(String),
	#[error("config: {0}")]
	Config(String),
}

#[cfg(test)]
mod tests {
	use super::is_idempotent;

	#[test]
	fn recognises_idempotent_broadcast_responses() {
		assert!(is_idempotent("already known"));
		assert!(is_idempotent("nonce too low"));
		assert!(is_idempotent("replacement transaction underpriced"));
		assert!(!is_idempotent("insufficient funds for gas * price + value"));
		assert!(!is_idempotent("execution reverted"));
	}
}
