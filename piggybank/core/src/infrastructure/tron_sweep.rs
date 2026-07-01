//! Tron treasury sweep — consolidates user TRC20 deposit balances into the treasury hot wallet.
//!
//! The Tron analogue of [`sweep`](super::sweep): a deposit credits the user's ledger claim, but the
//! USDT physically lands on the user's derived deposit address; this moves it to the treasury that
//! pays withdrawals. A user address holds only USDT, and a TRC20 transfer burns TRX for
//! energy/bandwidth, so a **gas station** (`Uuid::from_u128(1)`) tops the address up with TRX
//! first; the next cycle sweeps the USDT.
//!
//! Idempotency without a nonce. Tron has no nonce, so a re-sign is NOT byte-identical (it gets a
//! fresh ref-block + txID). Two on-chain facts carry safety instead:
//!   - the on-chain **balance is the truth** — once a sweep lands in a block, `account_state` reads
//!     zero USDT and the address drops out, so nothing re-sweeps;
//!   - a double-send cannot double-spend: a second sweep of the same address would move the same
//!     balance, but the first already moved it, so the second fails validation (insufficient
//!     balance) and merely wastes a fee. A short in-memory grace makes even that rare.
//!
//! Residual: an in-memory grace is lost on restart, so a restart in the ~seconds between a sweep's
//! broadcast and its inclusion can cost one reverted (no-double-spend) re-send — an accepted
//! operational cost, like the EVM sweep's stuck-tx residual.
//!
//! Read-mostly and opt-in (`SWEEP_ENABLED`): never touches TigerBeetle, off unless the operator
//! funds the gas station and turns it on. Scope: TRC20 only.

use std::{
	collections::HashMap,
	sync::Mutex,
	time::{Duration, Instant},
};

use evbanking_auth::ServiceTokenSource;
use evbanking_contracts::signer::v1::{ProvisionAddressRequest, SignTrc20TransferRequest, SignTrxTransferRequest, signer_service_client::SignerServiceClient};
use sqlx::PgPool;
use tokio::sync::OnceCell;
use tokio_util::sync::CancellationToken;
use tonic::{Request, transport::Channel};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{
	config::{TronConfig, TronSweepConfig},
	infrastructure::tron_rpc::{RefBlockParams, TronRpc, TronRpcError},
};

/// The reserved gas-station account id, distinct from the nil treasury: a wallet holding only TRX,
/// used to top up user deposit addresses with fee budget before their USDT is swept.
const GAS_STATION: Uuid = Uuid::from_u128(1);

pub struct TronSweep {
	pool: PgPool,
	rpc: TronRpc,
	signer: SignerServiceClient<Channel>,
	service_token: Option<ServiceTokenSource>,
	usdt_contract: String,
	fee_limit: i64,
	config: TronSweepConfig,
	treasury: OnceCell<String>,
	gas_station: OnceCell<String>,
	state: Mutex<SweepState>,
}
impl TronSweep {
	pub fn new(pool: PgPool, channel: Channel, service_token: Option<ServiceTokenSource>, tron: &TronConfig, config: TronSweepConfig) -> Self {
		Self {
			pool,
			rpc: TronRpc::new(tron.rpc_url.clone(), tron.api_key.clone(), tron.expiration_secs),
			signer: SignerServiceClient::new(channel),
			service_token,
			usdt_contract: tron.usdt_contract.clone(),
			fee_limit: tron.fee_limit,
			config,
			treasury: OnceCell::new(),
			gas_station: OnceCell::new(),
			state: Mutex::new(SweepState::default()),
		}
	}

	pub async fn run(self, shutdown: CancellationToken) {
		info!(
			min_usdt = self.config.min_usdt,
			poll_secs = self.config.poll_secs,
			"tron sweep: consolidating TRC20 deposits into the treasury"
		);
		match (self.address(&self.treasury, Uuid::nil()).await, self.address(&self.gas_station, GAS_STATION).await) {
			(Ok(treasury), Ok(gas_station)) => info!(%treasury, %gas_station, "tron sweep: fund the gas station with TRX — it pays the fee to sweep user USDT into the treasury"),
			_ => warn!("tron sweep: could not resolve the treasury/gas-station addresses yet (will retry each cycle)"),
		}
		loop {
			if let Err(err) = self.sweep_once().await {
				warn!("tron sweep: cycle failed, retrying next poll: {err}");
			}
			tokio::select! {
				() = shutdown.cancelled() => {
					info!("tron sweep: shutdown requested — stopping");
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
			if address == treasury || address == gas_station {
				continue; // never sweep a system wallet into itself.
			}
			if let Err(err) = self.sweep_address(user_id, &address, &treasury).await {
				warn!(%address, "tron sweep: address cycle failed (continuing): {err}");
			}
		}
		Ok(())
	}

	async fn sweep_address(&self, user_id: Uuid, address: &str, treasury: &str) -> Result<(), SweepError> {
		let state = self.rpc.account_state(address).await.map_err(read_err)?;
		let usdt = state.trc20(&self.usdt_contract);
		if usdt < self.config.min_usdt {
			return Ok(());
		}
		// Grace: a sweep we sent moments ago may not be in a block yet, so the balance still reads
		// non-zero — don't sign a second one. After the grace, a still-funded address re-sweeps.
		if self.recently(address, |s| &mut s.recent_sweeps, self.config.poll_secs.max(90)) {
			return Ok(());
		}
		if state.trx < self.config.min_trx_drop_sun {
			// Not enough TRX to pay the transfer's energy/bandwidth — top up, sweep next cycle.
			return self.top_up_gas(address).await;
		}
		let refs = self.rpc.ref_block_params().await.map_err(read_err)?;
		let signed = self.sign_sweep(user_id, address, treasury, usdt, &refs).await?;
		self.mark(address, |s| &mut s.recent_sweeps);
		self.broadcast(&signed, "sweep", address).await;
		Ok(())
	}

	async fn top_up_gas(&self, address: &str) -> Result<(), SweepError> {
		if self.recently(address, |s| &mut s.recent_topups, self.config.topup_grace_secs) {
			return Ok(());
		}
		// The signer resolves the gas-station key from its reserved id, so the address isn't needed.
		let refs = self.rpc.ref_block_params().await.map_err(read_err)?;
		let signed = self.sign_native(address, self.config.min_trx_drop_sun, &refs).await?;
		self.mark(address, |s| &mut s.recent_topups);
		info!(%address, drop_sun = self.config.min_trx_drop_sun, "tron sweep: topping up TRX for a deposit address");
		self.broadcast(&signed, "gas", address).await;
		Ok(())
	}

	async fn sign_sweep(&self, user_id: Uuid, address: &str, treasury: &str, amount: u128, refs: &RefBlockParams) -> Result<String, SweepError> {
		let mut request = Request::new(SignTrc20TransferRequest {
			from_user_id: user_id.to_string(), // the deposit address's owner — the signer holds its key
			network: "trc20".to_owned(),
			token_contract: self.usdt_contract.clone(),
			to_address: treasury.to_owned(),
			amount: amount.to_string(),
			ref_block_bytes: refs.ref_block_bytes.clone(),
			ref_block_hash: refs.ref_block_hash.clone(),
			expiration: refs.expiration,
			timestamp: refs.timestamp,
			fee_limit: self.fee_limit,
		});
		if let Some(token) = &self.service_token {
			request = token.authorize(request);
		}
		let response = self
			.signer
			.clone()
			.sign_trc20_transfer(request)
			.await
			.map_err(|s| SweepError::Signer(format!("sweep {address}: {}", s.message())))?
			.into_inner();
		Ok(response.signed_tx)
	}

	async fn sign_native(&self, to: &str, amount: u128, refs: &RefBlockParams) -> Result<String, SweepError> {
		let mut request = Request::new(SignTrxTransferRequest {
			from_user_id: GAS_STATION.to_string(),
			network: "trc20".to_owned(),
			to_address: to.to_owned(),
			amount: amount.to_string(),
			ref_block_bytes: refs.ref_block_bytes.clone(),
			ref_block_hash: refs.ref_block_hash.clone(),
			expiration: refs.expiration,
			timestamp: refs.timestamp,
		});
		if let Some(token) = &self.service_token {
			request = token.authorize(request);
		}
		let response = self
			.signer
			.clone()
			.sign_trx_transfer(request)
			.await
			.map_err(|s| SweepError::Signer(format!("gas top-up {to}: {}", s.message())))?
			.into_inner();
		Ok(response.signed_tx)
	}

	/// Broadcast a signed transaction, classifying the outcome per-address (never fails the whole
	/// cycle): a duplicate is benign, a sender out of funds is a loud alert (fund the gas station /
	/// treasury), a transport blip retries next cycle.
	async fn broadcast(&self, signed_tx_hex: &str, kind: &str, address: &str) {
		match self.rpc.broadcast_hex(signed_tx_hex).await {
			Ok(txid) => info!(%txid, kind, %address, "tron sweep: broadcast transaction"),
			Err(TronRpcError::Transport(detail)) => warn!(kind, %address, "tron sweep: transport error (retry next cycle): {detail}"),
			Err(TronRpcError::Rpc(msg)) if is_idempotent(&msg) => info!(kind, %address, reason = %msg, "tron sweep: transaction already in flight — idempotent"),
			Err(TronRpcError::Rpc(msg)) if is_insufficient(&msg) => error!(kind, %address, "tron sweep: SENDER OUT OF FUNDS — fund the gas station (TRX) / treasury: {msg}"),
			Err(TronRpcError::Rpc(msg)) => warn!(kind, %address, "tron sweep: node rejected the transaction: {msg}"),
		}
	}

	/// A system wallet's Tron address, resolved once via `ProvisionAddress` (`Uuid::nil()` =
	/// treasury, [`GAS_STATION`] = gas station) and cached.
	async fn address(&self, cell: &OnceCell<String>, id: Uuid) -> Result<String, SweepError> {
		cell.get_or_try_init(|| async {
			let mut request = Request::new(ProvisionAddressRequest {
				user_id: id.to_string(),
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
		sqlx::query_as::<_, (Uuid, String)>("SELECT user_id, address FROM user_deposit_addresses WHERE network = 'trc20' AND address_kind = 'derived'")
			.fetch_all(&self.pool)
			.await
			.map_err(|e| SweepError::Db(e.to_string()))
	}

	/// True if `select` last fired for `address` within `grace_secs` — a best-effort in-memory
	/// dedup so we don't pile a second top-up / sweep onto an address while the first is landing.
	fn recently(&self, address: &str, select: impl Fn(&mut SweepState) -> &mut HashMap<String, Instant>, grace_secs: u64) -> bool {
		let mut state = match self.state.lock() {
			Ok(state) => state,
			Err(_) => return false,
		};
		select(&mut state).get(address).is_some_and(|at| at.elapsed() < Duration::from_secs(grace_secs))
	}

	fn mark(&self, address: &str, select: impl Fn(&mut SweepState) -> &mut HashMap<String, Instant>) {
		if let Ok(mut state) = self.state.lock() {
			select(&mut state).insert(address.to_owned(), Instant::now());
		}
	}
}

#[derive(Default)]
struct SweepState {
	/// Last time TRX was topped up to an address (grace against piling on while it confirms).
	recent_topups: HashMap<String, Instant>,
	/// Last time a USDT sweep was sent from an address (grace against a second sweep before the
	/// first lands and zeroes the balance).
	recent_sweeps: HashMap<String, Instant>,
}

/// Node responses meaning our transaction is already accounted for — a re-send is a no-op.
fn is_idempotent(msg: &str) -> bool {
	let m = msg.to_uppercase();
	m.contains("DUP_TRANSACTION") || m.contains("ALREADY EXISTS") || m.contains("ALREADY KNOWN")
}

/// The sender can't cover the transfer or its fee — fund the gas station / treasury.
fn is_insufficient(msg: &str) -> bool {
	let m = msg.to_lowercase();
	m.contains("balance is not sufficient") || m.contains("insufficient") || m.contains("out_of_energy") || m.contains("out of energy")
}

fn read_err(err: TronRpcError) -> SweepError {
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
	use super::{is_idempotent, is_insufficient};

	#[test]
	fn classifies_broadcast_responses() {
		assert!(is_idempotent("DUP_TRANSACTION_ERROR: dup transaction"));
		assert!(!is_idempotent("CONTRACT_VALIDATE_ERROR: balance is not sufficient"));
		assert!(is_insufficient("CONTRACT_VALIDATE_ERROR: balance is not sufficient"));
		assert!(is_insufficient("account does not have enough energy(OUT_OF_ENERGY)"));
		assert!(!is_insufficient("DUP_TRANSACTION_ERROR"));
	}
}
