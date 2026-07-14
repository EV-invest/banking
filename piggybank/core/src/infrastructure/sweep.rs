//! Treasury sweep — consolidates user deposit balances into the treasury hot wallet.
//!
//! A deposit credits the user's ledger claim ([`deposit_watcher`](super::deposit_watcher)),
//! but the USDT physically lands on the user's *derived deposit address*, not in the
//! treasury that pays withdrawals. This background job moves it: for each deposit address
//! holding USDT, it signs an ERC-20 transfer **from that address** (the signer holds the
//! key) to the treasury and broadcasts it — so deposits become treasury liquidity.
//!
//! **Gas.** A user address holds only USDT, and an ERC-20 transfer costs native coin (BNB on
//! BEP20, POL on Polygon). So a separate **gas station** account (its own reserved id ⇒ its own
//! nonce sequence, independent of the treasury's — the withdrawal path is never raced for a nonce)
//! tops the address up with a little native coin first; the next cycle, with gas present, sweeps the USDT.
//!
//! **Idempotency (no double-send), with no extra persistence.** Three on-chain facts carry it:
//!   - the USDT sweep is signed at the address's **mined** (`latest`) nonce, so a re-sign of
//!     an unconfirmed sweep is the *same* deterministic transaction (RFC-6979) — an
//!     idempotent "already known" re-broadcast, never a second tx at the next nonce;
//!   - the on-chain **balance** is the truth — once a sweep mines, the address reads zero and
//!     drops out, so nothing re-sweeps;
//!   - the gas station's nonce is an in-memory monotonic counter (seeded from the chain), so
//!     several top-ups in one cycle don't collide, plus a short in-memory grace so we don't
//!     pile up top-ups to one address while the first confirms. A nonce whose top-up is not
//!     known to have reached the mempool (signer failure, node rejection, transport failure)
//!     is freed for the next cycle — a consumed-but-absent nonce would gap the sequence and
//!     queue every later top-up behind a slot nothing fills, wedging the sweep until restart.
//!
//! Read-mostly and **opt-in** (`SWEEP_ENABLED`, or the per-rail `<RAIL>_SWEEP_ENABLED`): it never
//! touches TigerBeetle (the deposit was already credited; this only relocates the on-chain custody),
//! and it is off unless the operator funds the gas station and turns it on. Scope: the EVM rails
//! (BEP20, Polygon) — one instance per rail, keyed by `network`. It operates in on-chain raw units
//! throughout (moving the address's raw balance), so the per-rail `min_usdt` floor is expressed at
//! the chain's own precision (18-dp on BEP20, 6-dp on Polygon). A stuck underpriced transaction (no
//! replacement-by-fee here) blocking a nonce is a known operational residual — manual intervention,
//! like the reaper's stuck-withdrawal alert.

use std::{
	collections::HashMap,
	sync::Mutex,
	time::{Duration, Instant},
};

use domain::money::Network;
use evbanking_auth::ServiceTokenSource;
use evbanking_contracts::signer::v1::{ProvisionAddressRequest, SignErc20TransferRequest, SignNativeTransferRequest, signer_service_client::SignerServiceClient};
use sqlx::PgPool;
use tokio::sync::OnceCell;
use tokio_util::sync::CancellationToken;
use tonic::{Request, transport::Channel};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{
	config::{EvmConfig, SweepConfig},
	infrastructure::evm_rpc::{EvmRpc, RpcError},
};

/// The reserved gas-station account id, distinct from the nil treasury: a wallet holding
/// only native coin (BNB/POL), used to top up user deposit addresses with gas. A separate account
/// means a separate nonce sequence, so the sweep never races the withdrawal custody path.
const GAS_STATION: Uuid = Uuid::from_u128(1);

/// 1 gwei in wei. Gas prices are rounded UP to a whole gwei so minor node-to-node wobble
/// doesn't change a re-signed transaction's bytes (keeping the idempotent-re-sign property).
const GWEI: u128 = 1_000_000_000;

/// Gas for a plain native (BNB/POL) value transfer.
const NATIVE_TRANSFER_GAS: u64 = 21_000;

pub struct Sweep {
	network: Network,
	pool: PgPool,
	rpc: EvmRpc,
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
	pub fn new(pool: PgPool, channel: Channel, service_token: Option<ServiceTokenSource>, evm: &EvmConfig, config: SweepConfig) -> Self {
		Self {
			network: evm.network,
			pool,
			rpc: EvmRpc::new(evm.rpc_url.clone()),
			signer: SignerServiceClient::new(channel),
			service_token,
			usdt_contract: evm.usdt_contract.clone(),
			chain_id: evm.chain_id,
			transfer_gas_limit: evm.gas_limit,
			config,
			treasury: OnceCell::new(),
			gas_station: OnceCell::new(),
			state: Mutex::new(GasState::default()),
		}
	}

	pub async fn run(self, shutdown: CancellationToken) {
		info!(
			network = %self.network,
			min_usdt = self.config.min_usdt,
			poll_secs = self.config.poll_secs,
			"sweep: consolidating EVM deposits into the treasury"
		);
		// Resolve + log the system wallets up front so the operator can fund the gas station
		// with native coin (it pays the gas to move user USDT). Best-effort — retried each cycle.
		match (self.address(&self.treasury, Uuid::nil()).await, self.address(&self.gas_station, GAS_STATION).await) {
			(Ok(treasury), Ok(gas_station)) =>
				info!(network = %self.network, %treasury, %gas_station, "sweep: fund the gas station with native coin (BNB/POL) — it pays gas to sweep user USDT into the treasury"),
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
			// Drained: the consolidation mined (or the deposit was dust). Stamp the
			// user's credited deposits so this address drops out of the scan until a
			// NEW deposit is credited — the fix for the O(N)-every-cycle RPC melt.
			self.mark_swept(user_id).await?;
			return Ok(());
		}
		let gas_price = self.gas_price().await?;
		let needed = gas_price.saturating_mul(self.transfer_gas_limit as u128);
		let bnb = self.rpc.native_balance(address).await.map_err(read_err)?;
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
		let (raw, hash) = match self.sign_native(address, drop, nonce, gas_price).await {
			Ok(signed) => signed,
			Err(err) => {
				self.free_gas_nonce(nonce);
				return Err(err);
			}
		};
		// Record before broadcasting so a slow/failed send still dedups the next cycle.
		if let Ok(mut state) = self.state.lock() {
			state.recent_topups.insert(address.to_owned(), Instant::now());
		}
		info!(%address, drop, "sweep: topping up gas for a deposit address");
		match self.broadcast(&raw, &hash, "gas", address).await {
			Broadcast::Admitted => {}
			Broadcast::Rejected => {
				// In no mempool anywhere: free the slot and the grace, so the next cycle
				// retries at once (the grace only exists to wait out a confirming tx).
				self.free_gas_nonce(nonce);
				if let Ok(mut state) = self.state.lock() {
					state.recent_topups.remove(address);
				}
			}
			// The send may have reached the node: free the slot (the allocator re-syncs from
			// the chain's pending count if the tx did get through), but KEEP the grace — it
			// is what stops a second drop to this address while the first may be confirming.
			Broadcast::Ambiguous => self.free_gas_nonce(nonce),
		}
		Ok(())
	}

	/// Roll the in-memory high-water mark back for a top-up that is not known to have entered
	/// the mempool, so the next cycle retries the same slot — the withdrawal path's
	/// `discard_tx` discipline. Without this the chain's pending count stays pinned at the
	/// gap while every later top-up signs above it into the queued pool, and the sweep wedges
	/// for the life of the process. Safe even when a transport-ambiguous send actually got
	/// through: [`next_gas_nonce`](Self::next_gas_nonce) never allocates below the chain's
	/// pending count, so an admitted tx re-syncs the sequence, and at worst a duplicate
	/// signing at the same nonce is classified idempotent at broadcast.
	fn free_gas_nonce(&self, nonce: u64) {
		if let Ok(mut state) = self.state.lock() {
			state.free(nonce);
		}
	}

	/// The next gas-station nonce: the chain's pending count, but never below our in-memory
	/// high-water mark, so several top-ups within one cycle (or before the node's pending
	/// count catches up) get distinct, monotonic nonces.
	async fn next_gas_nonce(&self, gas_station: &str) -> Result<u64, SweepError> {
		let chain = self.rpc.pending_nonce(gas_station).await.map_err(read_err)?;
		let mut state = self.state.lock().map_err(|_| SweepError::Config("sweep state mutex poisoned".into()))?;
		Ok(state.allocate(chain))
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
			network: self.network.as_str().to_owned(),
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
			.map_err(|s| {
				super::telemetry::note_signer_error("sweep", address, s.message());
				SweepError::Signer(format!("sweep {address}: {}", s.message()))
			})?
			.into_inner();
		Ok((response.raw_tx, response.tx_hash))
	}

	async fn sign_native(&self, to: &str, amount: u128, nonce: u64, gas_price: u128) -> Result<(String, String), SweepError> {
		let mut request = Request::new(SignNativeTransferRequest {
			from_user_id: GAS_STATION.to_string(),
			network: self.network.as_str().to_owned(),
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
			.map_err(|s| {
				super::telemetry::note_signer_error("gas top-up", "gas-station", s.message());
				SweepError::Signer(format!("gas top-up {to}: {}", s.message()))
			})?
			.into_inner();
		Ok((response.raw_tx, response.tx_hash))
	}

	/// Broadcast a signed transaction, classifying the outcome per-address (never fails the
	/// whole cycle): an idempotent re-send is benign, a sender out of funds is a loud alert
	/// (fund the gas station / treasury), a transport blip retries next cycle. The returned
	/// [`Broadcast`] lets the gas path decide whether the nonce it consumed must be freed.
	async fn broadcast(&self, raw_tx: &str, tx_hash: &str, kind: &str, address: &str) -> Broadcast {
		match self.rpc.send_raw_transaction(raw_tx).await {
			Ok(hash) => {
				info!(%hash, kind, %address, "sweep: broadcast transaction");
				Broadcast::Admitted
			}
			Err(RpcError::Transport(detail)) => {
				warn!(kind, %address, "sweep: transport error (retry next cycle): {detail}");
				Broadcast::Ambiguous
			}
			Err(RpcError::Rpc(msg)) if is_idempotent(&msg) => {
				info!(kind, %address, reason = %msg, "sweep: transaction already in flight — idempotent");
				Broadcast::Admitted
			}
			Err(RpcError::Rpc(msg)) if msg.to_lowercase().contains("insufficient funds") => {
				error!(network = %self.network, kind, %address, %tx_hash, "sweep: SENDER OUT OF FUNDS — fund the gas station (BNB/POL) / treasury: {msg}");
				Broadcast::Rejected
			}
			Err(RpcError::Rpc(msg)) => {
				warn!(kind, %address, "sweep: node rejected the transaction: {msg}");
				Broadcast::Rejected
			}
		}
	}

	/// A system wallet's on-chain address for this rail, resolved once via `ProvisionAddress`
	/// (`Uuid::nil()` = treasury, [`GAS_STATION`] = gas station) and cached. A transient failure
	/// leaves the cell empty so a later cycle retries.
	async fn address(&self, cell: &OnceCell<String>, id: Uuid) -> Result<String, SweepError> {
		cell.get_or_try_init(|| async {
			let mut request = Request::new(ProvisionAddressRequest {
				user_id: id.to_string(),
				network: self.network.as_str().to_owned(),
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

	/// Addresses that can still hold funds: a credited deposit exists that no sweep
	/// cycle has yet observed drained. Everything else is skipped without an RPC —
	/// the scan is O(active deposits), not O(all addresses ever provisioned).
	async fn deposit_addresses(&self) -> Result<Vec<(Uuid, String)>, SweepError> {
		sqlx::query_as::<_, (Uuid, String)>(
			"SELECT DISTINCT a.user_id, a.address FROM deposits d \
			 JOIN user_deposit_addresses a ON a.user_id::text = d.party_id AND a.network = d.network \
			 WHERE d.network = $1 AND d.party_kind = 'user' AND d.swept_at IS NULL AND a.address_kind = 'derived'",
		)
		.bind(self.network.as_str())
		.fetch_all(&self.pool)
		.await
		.map_err(|e| SweepError::Db(e.to_string()))
	}

	async fn mark_swept(&self, user_id: Uuid) -> Result<(), SweepError> {
		sqlx::query("UPDATE deposits SET swept_at = now() WHERE party_kind = 'user' AND party_id = $1 AND network = $2 AND swept_at IS NULL")
			.bind(user_id.to_string())
			.bind(self.network.as_str())
			.execute(&self.pool)
			.await
			.map_err(|e| SweepError::Db(e.to_string()))?;
		Ok(())
	}
}

#[derive(Default)]
struct GasState {
	/// In-memory monotonic next nonce for the gas station, so several top-ups in one cycle
	/// get distinct nonces even when the node's pending count lags. Seeded/bumped from the
	/// chain; only goes backwards via [`free`](Self::free) when an allocated nonce's
	/// transaction never entered the mempool.
	next_nonce: Option<u64>,
	/// Last time a top-up was sent to an address — a best-effort grace so we don't pile up
	/// top-ups while one confirms (in-memory; a restart may cost one extra harmless top-up).
	recent_topups: HashMap<String, Instant>,
}
impl GasState {
	/// Consume the next nonce: the chain's pending count, but never below the in-memory
	/// high-water mark.
	fn allocate(&mut self, chain: u64) -> u64 {
		let next = self.next_nonce.map_or(chain, |n| n.max(chain));
		self.next_nonce = Some(next + 1);
		next
	}

	/// Return the most recently allocated nonce to the pool. Only the latest allocation can
	/// be freed — an older one has live transactions signed above it, so re-opening it would
	/// trade one gap for another.
	fn free(&mut self, nonce: u64) {
		if self.next_nonce == Some(nonce + 1) {
			self.next_nonce = Some(nonce);
		}
	}
}

/// The classified outcome of a broadcast, as far as mempool admission is concerned.
enum Broadcast {
	/// In the mempool: accepted now, or an idempotent re-send of one already there.
	Admitted,
	/// The node synchronously rejected it — these bytes are in no mempool anywhere.
	Rejected,
	/// A transport failure: the send may or may not have reached the node.
	Ambiguous,
}

/// Node responses that mean our transaction is already accounted for — a re-send is a no-op.
/// `nonce too low` ⇒ it already mined; `already known` ⇒ already in the mempool;
/// `replacement transaction underpriced` ⇒ the prior identical-nonce tx still stands (this
/// also safely covers a *different* tx colliding at a reused nonce after an ambiguous send:
/// the earlier tx stands, and the loser is simply retried on a later cycle).
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
	use super::{GasState, is_idempotent};

	#[test]
	fn recognises_idempotent_broadcast_responses() {
		assert!(is_idempotent("already known"));
		assert!(is_idempotent("nonce too low"));
		assert!(is_idempotent("replacement transaction underpriced"));
		assert!(!is_idempotent("insufficient funds for gas * price + value"));
		assert!(!is_idempotent("execution reverted"));
	}

	#[test]
	fn allocates_monotonic_nonces_from_the_chain() {
		let mut state = GasState::default();
		assert_eq!(state.allocate(10), 10);
		assert_eq!(state.allocate(10), 11, "chain pending lags within a cycle — high-water mark wins");
		assert_eq!(state.allocate(20), 20, "chain moved past us (external mining) — chain wins");
	}

	#[test]
	fn freeing_a_failed_topup_retries_the_same_slot() {
		let mut state = GasState::default();
		let nonce = state.allocate(10);
		state.free(nonce);
		assert_eq!(state.allocate(10), nonce, "the freed nonce is reissued, not skipped");
	}

	#[test]
	fn freed_nonce_resyncs_when_the_tx_was_admitted_after_all() {
		let mut state = GasState::default();
		let nonce = state.allocate(10);
		state.free(nonce); // transport-ambiguous send that actually got through
		assert_eq!(state.allocate(11), 11, "chain pending advanced — no duplicate allocation");
	}

	#[test]
	fn only_the_latest_allocation_can_be_freed() {
		let mut state = GasState::default();
		let first = state.allocate(10);
		let second = state.allocate(10);
		state.free(first);
		assert_eq!(state.allocate(10), second + 1, "an older nonce has live txs above it — freeing is a no-op");
	}
}
