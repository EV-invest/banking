//! Custody adapters — the relay's "broadcast this withdrawal" seam.
//!
//! [`StubCustody`] is the no-op stand-in (an operator settles manually). [`ChainCustody`]
//! is the real one: it signs the withdrawal's ERC-20 transfer via the signer (the key never
//! leaves there) and broadcasts it to the EVM chain. **Crash-safety / no double-spend:** the signed
//! transaction is persisted (`withdrawal_broadcasts`, keyed by `withdrawal_id`) BEFORE it is
//! sent, so an at-least-once relay re-delivery re-broadcasts the SAME bytes (same nonce)
//! rather than signing a new one — a withdrawal can never go out twice under two nonces.
//!
//! Scope: the EVM rails — one [`ChainCustody`] instance per rail (BEP20, Polygon), each carrying its
//! own `network` so their persisted nonce sequences and treasury keys stay disjoint. The token's
//! on-chain decimals differ (BEP20 18-dp, Polygon 6-dp); the balance edge normalizes via
//! [`Usdt::from_onchain`]. The on-chain SETTLE (reducing the ledger's rail custody) is a separate
//! step — an operator's `SettleWithdrawal` (or the confirmation watcher) on the mined
//! transaction; this adapter only gets the bytes onto the chain.

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use domain::{
	architecture::Gateway,
	money::{Network, Usdt},
};
use evbanking_auth::ServiceTokenSource;
use evbanking_contracts::signer::v1::{ProvisionAddressRequest, SignErc20TransferRequest, signer_service_client::SignerServiceClient};
use sqlx::PgPool;
use tokio::sync::OnceCell;
use tonic::{Request, transport::Channel};
use tracing::{info, warn};
use uuid::Uuid;

/// The reserved sweep gas-station wallet id (see `sweep.rs` — same convention): a
/// native-coin-only account (BNB/POL) whose funding view rides along on the treasury screen.
const GAS_STATION: Uuid = Uuid::from_u128(1);

use crate::{
	config::EvmConfig,
	infrastructure::evm_rpc::{EvmRpc, RpcError},
	ports::custody::{BroadcastRequest, Custody, CustodyError, TreasuryFunding, format_native_units},
};

/// No-op custody: logs and returns success. An operator supplies the real on-chain tx ref
/// later via `BalanceService.SettleWithdrawal`. Used when BSC is unconfigured.
pub struct StubCustody;

impl Gateway for StubCustody {}

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

/// Routes each withdrawal to the per-rail custody adapter registered for its network. The relay
/// holds a single `Arc<dyn Custody>`; this is that one — it fans out by `request.network`, so each
/// chain's adapter ([`ChainCustody`] for the EVM rails BEP20/Polygon, and the TRC20/TON equivalents)
/// stays single-rail and never has to know about the others. A network with no registered adapter falls through to
/// `fallback` (the [`StubCustody`]), so an unwired rail behaves like unconfigured custody — an
/// operator settles it manually — rather than hard-rejecting a real withdrawal.
pub struct MultiChainCustody {
	by_network: HashMap<Network, Arc<dyn Custody>>,
	fallback: Arc<dyn Custody>,
}

impl MultiChainCustody {
	pub fn new(by_network: HashMap<Network, Arc<dyn Custody>>, fallback: Arc<dyn Custody>) -> Self {
		Self { by_network, fallback }
	}
}

impl Gateway for MultiChainCustody {}

#[async_trait]
impl Custody for MultiChainCustody {
	async fn broadcast(&self, request: &BroadcastRequest) -> Result<(), CustodyError> {
		self.by_network.get(&request.network).unwrap_or(&self.fallback).broadcast(request).await
	}

	async fn treasury_liquidity(&self, network: Network) -> Result<Option<Usdt>, CustodyError> {
		// An unwired rail has no chain view (`None`) — it must behave like the stub
		// (operator-settled), not report a zero that would freeze its dispatches.
		match self.by_network.get(&network) {
			Some(adapter) => adapter.treasury_liquidity(network).await,
			None => Ok(None),
		}
	}

	async fn treasury_funding(&self, network: Network) -> Result<Option<TreasuryFunding>, CustodyError> {
		match self.by_network.get(&network) {
			Some(adapter) => adapter.treasury_funding(network).await,
			None => Ok(None),
		}
	}
}

/// Real EVM custody (BEP20 / Polygon): sign via the signer's treasury key, broadcast via the
/// node. One instance per EVM rail — `network` scopes its persisted nonce sequence and deposit
/// addresses so the two EVM rails never share a nonce space or a treasury key.
pub struct ChainCustody {
	network: Network,
	pool: PgPool,
	rpc: EvmRpc,
	signer: SignerServiceClient<Channel>,
	service_token: Option<ServiceTokenSource>,
	chain_id: u64,
	usdt_contract: String,
	gas_limit: u64,
	/// The treasury hot wallet's address (the withdrawal source), resolved once via the
	/// signer and cached. Funds — USDT to send, and native coin (BNB/POL) for gas — are
	/// deposited here out-of-band.
	treasury_address: OnceCell<String>,
	/// The sweep gas-station's address, resolved once for the operator funding view.
	gas_station_address: OnceCell<String>,
}

impl ChainCustody {
	pub fn new(pool: PgPool, evm: &EvmConfig, signer: SignerServiceClient<Channel>, service_token: Option<ServiceTokenSource>) -> Self {
		Self {
			network: evm.network,
			pool,
			rpc: EvmRpc::new(evm.rpc_url.clone()),
			signer,
			service_token,
			chain_id: evm.chain_id,
			usdt_contract: evm.usdt_contract.clone(),
			gas_limit: evm.gas_limit,
			treasury_address: OnceCell::new(),
			gas_station_address: OnceCell::new(),
		}
	}

	/// The treasury's on-chain address for this rail, resolved once via `ProvisionAddress` (the
	/// reserved nil user id) and cached. A transient failure leaves the cell empty so a later call
	/// retries. Public so the composition root can resolve + log it at boot — the operator funds
	/// this address out-of-band (USDT for liquidity, native coin for gas) before withdrawals settle.
	pub async fn treasury_address(&self) -> Result<String, CustodyError> {
		self.treasury_address
			.get_or_try_init(|| async {
				let mut request = Request::new(ProvisionAddressRequest {
					user_id: Uuid::nil().to_string(),
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
					.map_err(|s| CustodyError::Unavailable(format!("resolve treasury address: {}", s.message())))?
					.into_inner();
				if response.address_kind != "derived" {
					return Err(CustodyError::Rejected(format!("treasury address is not fundable (kind={})", response.address_kind)));
				}
				info!(treasury = %response.address, network = %self.network, "chain custody: treasury hot wallet — fund it with USDT (liquidity) + native gas");
				Ok(response.address)
			})
			.await
			.cloned()
	}

	/// The sweep gas-station's address, resolved once via `ProvisionAddress` (the
	/// reserved [`GAS_STATION`] id) and cached — read-only, for the funding view.
	async fn gas_station_address(&self) -> Result<String, CustodyError> {
		self.gas_station_address
			.get_or_try_init(|| async {
				let mut request = Request::new(ProvisionAddressRequest {
					user_id: GAS_STATION.to_string(),
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
					.map_err(|s| CustodyError::Unavailable(format!("resolve gas-station address: {}", s.message())))?
					.into_inner();
				if response.address_kind != "derived" {
					return Err(CustodyError::Rejected(format!("gas-station address is not fundable (kind={})", response.address_kind)));
				}
				Ok(response.address)
			})
			.await
			.cloned()
	}

	/// The previously signed+stored raw transaction for this withdrawal, if any.
	async fn stored_tx(&self, withdrawal_id: Uuid) -> Result<Option<String>, CustodyError> {
		sqlx::query_scalar::<_, String>("SELECT raw_tx FROM withdrawal_broadcasts WHERE withdrawal_id = $1 AND network = $2")
			.bind(withdrawal_id)
			.bind(self.network.as_str())
			.fetch_optional(&self.pool)
			.await
			.map_err(db_unavailable)
	}

	async fn store_tx(&self, withdrawal_id: Uuid, nonce: u64, raw_tx: &str, tx_hash: &str) -> Result<(), CustodyError> {
		sqlx::query("INSERT INTO withdrawal_broadcasts (withdrawal_id, network, nonce, raw_tx, tx_hash) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (withdrawal_id) DO NOTHING")
			.bind(withdrawal_id)
			.bind(self.network.as_str())
			.bind(nonce as i64)
			.bind(raw_tx)
			.bind(tx_hash)
			.execute(&self.pool)
			.await
			.map_err(db_unavailable)?;
		Ok(())
	}

	/// Forget the stored tx for a withdrawal whose FIRST send the node synchronously
	/// rejected: those bytes never entered any mempool, so freeing its nonce is safe —
	/// keeping the row would gap the sequence at that nonce and queue every later
	/// withdrawal behind a slot nothing will ever fill. Only legal on the fresh path;
	/// a rebroadcast's earlier attempt may have propagated, so its row must stay.
	async fn discard_tx(&self, withdrawal_id: Uuid) -> Result<(), CustodyError> {
		sqlx::query("DELETE FROM withdrawal_broadcasts WHERE withdrawal_id = $1 AND network = $2")
			.bind(withdrawal_id)
			.bind(self.network.as_str())
			.execute(&self.pool)
			.await
			.map_err(db_unavailable)?;
		Ok(())
	}

	/// On-chain Read-First before a nonce is allocated or anything is signed: the treasury
	/// must hold the USDT to send AND the native coin (BNB/POL) to pay the gas. The ledger's
	/// rail-liquidity check reads TigerBeetle — accounting, not the hot wallet's real balances
	/// (a lagging sweep or an out-of-band spend desyncs them) — and letting the node discover the
	/// shortfall at broadcast would reject AFTER a nonce was allocated. A shortfall parks
	/// (`Rejected`) rather than retries: an unbounded retry would wedge the single-worker
	/// drain behind an underfunded rail, freezing every other money movement. `request.amount`
	/// is canonical base units; the on-chain balance is normalized to the same via `from_onchain`
	/// so an 18-dp (BEP20) and a 6-dp (Polygon) rail compare like-for-like.
	async fn ensure_treasury_funded(&self, treasury: &str, request: &BroadcastRequest, gas_price: u128) -> Result<(), CustodyError> {
		let raw = self.rpc.erc20_balance(&self.usdt_contract, treasury).await.map_err(read_err)?;
		let usdt = Usdt::from_onchain(self.network, raw).map_err(|e| CustodyError::Unavailable(format!("treasury USDT balance not representable: {e}")))?;
		if usdt < request.amount {
			return Err(CustodyError::Rejected(format!(
				"treasury underfunded on-chain: {} < {} needed (canonical base units)",
				usdt.base_units(),
				request.amount.base_units()
			)));
		}
		let native = self.rpc.native_balance(treasury).await.map_err(read_err)?;
		let gas = u128::from(self.gas_limit).saturating_mul(gas_price);
		if native < gas {
			return Err(CustodyError::Rejected(format!("treasury gas underfunded on-chain: {native} wei < {gas} needed")));
		}
		Ok(())
	}

	/// The next nonce for the treasury: the max of the chain's pending count and one past the
	/// highest nonce we've already assigned — monotonic even if a public node lags, and it
	/// catches up to the chain after a restart. Scoped to this rail's `network` so the two EVM
	/// rails' nonce sequences (and the seqno values other rails store in the shared `nonce`
	/// column) never bleed into each other.
	async fn next_nonce(&self, treasury: &str) -> Result<u64, CustodyError> {
		let chain = self.rpc.pending_nonce(treasury).await.map_err(read_err)?;
		let local_max: Option<i64> = sqlx::query_scalar("SELECT MAX(nonce) FROM withdrawal_broadcasts WHERE network = $1")
			.bind(self.network.as_str())
			.fetch_one(&self.pool)
			.await
			.map_err(db_unavailable)?;
		let local_next = local_max.map(|n| n as u64 + 1).unwrap_or(0);
		Ok(chain.max(local_next))
	}

	/// Sign the withdrawal's USDT transfer from the treasury key via the signer (the signer
	/// resolves the treasury key itself from the empty `from_user_id`).
	async fn sign(&self, request: &BroadcastRequest, nonce: u64, gas_price: u128) -> Result<(String, String), CustodyError> {
		let onchain_amount = onchain_transfer_amount(self.network, request.amount)?;
		let mut signer_request = Request::new(SignErc20TransferRequest {
			from_user_id: String::new(), // empty ⇒ treasury hot wallet
			network: self.network.as_str().to_owned(),
			token_contract: self.usdt_contract.clone(),
			to_address: request.address.as_str().to_owned(),
			amount: onchain_amount.to_string(),
			chain_id: self.chain_id,
			nonce,
			gas_price: gas_price.to_string(),
			gas_limit: self.gas_limit,
		});
		if let Some(token) = &self.service_token {
			signer_request = token.authorize(signer_request);
		}
		let response = self.signer.clone().sign_erc20_transfer(signer_request).await.map_err(|s| {
			super::telemetry::note_signer_error("withdrawal", "treasury", s.message());
			match s.code() {
				tonic::Code::Unavailable | tonic::Code::DeadlineExceeded => CustodyError::Unavailable(format!("signer: {}", s.message())),
				_ => CustodyError::Rejected(format!("signer: {}", s.message())),
			}
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

impl Gateway for ChainCustody {}

#[async_trait]
impl Custody for ChainCustody {
	async fn broadcast(&self, request: &BroadcastRequest) -> Result<(), CustodyError> {
		debug_assert!(
			request.network == self.network,
			"ChainCustody for {} must not be routed a {} withdrawal",
			self.network,
			request.network
		);
		// Idempotent: if we already signed+stored a transaction for this withdrawal, re-send
		// THOSE exact bytes rather than signing a new one (no second nonce can ever go out).
		if let Some(raw_tx) = self.stored_tx(request.withdrawal_id).await? {
			return self.submit(&raw_tx, true).await;
		}

		let treasury = self.treasury_address().await?;
		let gas_price = self.rpc.gas_price().await.map_err(read_err)?;
		// Balance Read-First BEFORE the nonce exists — an underfunded treasury parks the
		// withdrawal without ever burning a slot in the nonce sequence.
		self.ensure_treasury_funded(&treasury, request, gas_price).await?;
		let nonce = self.next_nonce(&treasury).await?;
		let (raw_tx, tx_hash) = self.sign(request, nonce, gas_price).await?;

		// Persist BEFORE broadcasting — a crash after this re-broadcasts THIS tx (same nonce),
		// never a freshly-signed one with a different nonce.
		self.store_tx(request.withdrawal_id, nonce, &raw_tx, &tx_hash).await?;
		match self.submit(&raw_tx, false).await {
			// The node synchronously refused a first-ever send: nothing entered the mempool,
			// so free the nonce before parking — otherwise the sequence gaps at this slot and
			// every later withdrawal signs above a hole nothing will ever fill.
			Err(err @ CustodyError::Rejected(_)) => {
				if let Err(discard) = self.discard_tx(request.withdrawal_id).await {
					warn!(withdrawal_id = %request.withdrawal_id, "chain custody: could not free the rejected tx's nonce (manual gap repair needed): {discard}");
				}
				Err(err)
			}
			other => other,
		}
	}

	async fn treasury_liquidity(&self, _network: Network) -> Result<Option<Usdt>, CustodyError> {
		let treasury = self.treasury_address().await?;
		let raw = self.rpc.erc20_balance(&self.usdt_contract, &treasury).await.map_err(read_err)?;
		// Scale the on-chain raw balance into canonical 18-dp base units per this rail's token
		// precision — BEP20 USDT is 18-dp (1:1), Polygon USDT is 6-dp (×10^12). Using
		// `from_base_units` here would over-count a 6-dp rail's treasury by 10^12. An overflow
		// (absurd for a real balance) degrades to an Err → the dispatch gate treats it as
		// "no chain view" and stays conservative (queues), never a wrong dispatch.
		let usdt = Usdt::from_onchain(self.network, raw).map_err(|e| CustodyError::Unavailable(format!("treasury USDT balance not representable: {e}")))?;
		Ok(Some(usdt))
	}

	async fn treasury_funding(&self, network: Network) -> Result<Option<TreasuryFunding>, CustodyError> {
		let address = self.treasury_address().await?;
		// Balance reads degrade to None (the address alone is still fundable); only an
		// address-resolution failure errors — the rail is then genuinely unavailable.
		let onchain_usdt = self.treasury_liquidity(network).await.ok().flatten();
		let onchain_gas = self.rpc.native_balance(&address).await.ok().map(|wei| format_native_units(wei, 18));
		// The gas station rides along best-effort: it must be visible on the treasury
		// screen so the operator funds the RIGHT wallet, but its failure must never
		// hide the treasury view itself.
		let gas_station_address = self.gas_station_address().await.ok();
		let gas_station_gas = match &gas_station_address {
			Some(gas_station) => self.rpc.native_balance(gas_station).await.ok().map(|wei| format_native_units(wei, 18)),
			None => None,
		};
		Ok(Some(TreasuryFunding {
			address,
			onchain_usdt,
			onchain_gas,
			gas_station_address,
			gas_station_gas,
		}))
	}
}

/// The raw on-chain ERC-20 `transfer` value for a canonical withdrawal amount on `network`.
/// The signer uses this integer VERBATIM (it does NOT rescale by decimals), so the canonical
/// 18-dp amount must be scaled DOWN to the token's on-chain precision here: 1:1 for BEP20 (18-dp),
/// ÷10^12 for Polygon (6-dp). Sending `base_units()` on a 6-dp rail would transfer 10^12× too much
/// (an on-chain revert that wedges the nonce). Sub-precision dust — never representable at the
/// chain's precision — is rejected (the withdrawal policy already bars it at request time).
fn onchain_transfer_amount(network: Network, amount: Usdt) -> Result<u128, CustodyError> {
	amount
		.to_onchain(network)
		.map_err(|e| CustodyError::Rejected(format!("withdrawal amount not representable on {network}: {e}")))
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
	use domain::money::{Network, Usdt};

	use super::{already_accepted, onchain_transfer_amount};

	#[test]
	fn recognises_already_submitted_responses() {
		assert!(already_accepted("already known"));
		assert!(already_accepted("ALREADY KNOWN"));
		assert!(already_accepted("known transaction: 0xabc"));
		assert!(already_accepted("transaction already imported"));
		assert!(!already_accepted("insufficient funds for gas * price + value"));
		assert!(!already_accepted("nonce too low"));
	}

	#[test]
	fn signs_the_transfer_at_the_rail_on_chain_precision() {
		// 50 USDT canonical (50e18 18-dp base units).
		let fifty = Usdt::from_base_units(50_000_000_000_000_000_000);
		// BEP20 USDT is 18-dp: the raw transfer value equals the canonical base units 1:1.
		assert_eq!(onchain_transfer_amount(Network::Bep20, fifty).unwrap(), 50_000_000_000_000_000_000);
		// Polygon USDT is 6-dp: the raw transfer value MUST be scaled down by 10^12 — the bug this
		// guards is signing `base_units()` (50e18) against a 6-dp token, which would send 10^12× too much.
		assert_eq!(onchain_transfer_amount(Network::Polygon, fifty).unwrap(), 50_000_000);
		// Sub-precision dust (1 canonical base unit) is not representable on a 6-dp rail → rejected.
		assert!(onchain_transfer_amount(Network::Polygon, Usdt::from_base_units(1)).is_err());
		assert!(onchain_transfer_amount(Network::Bep20, Usdt::from_base_units(1)).is_ok());
	}
}
