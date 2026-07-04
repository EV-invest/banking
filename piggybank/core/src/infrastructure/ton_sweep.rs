//! TON treasury sweep — consolidates user jetton (USDT) deposit balances into the
//! treasury. The TON sibling of [`sweep`](super::sweep).
//!
//! A deposit credits the user's ledger claim ([`ton_deposit_watcher`](super::ton_deposit_watcher)),
//! but the USDT physically lands in the user's derived wallet's *jetton wallet*. This job
//! moves it to the treasury in a **two-message choreography**, mirroring the EVM sweep's
//! gas-station pattern but adapted to TON's contract-wallet + seqno model:
//!   1. **Gas top-up.** The user's v4R2 wallet pays its own gas in TON, so a separate
//!      **gas-station** wallet first sends it a little Toncoin (a non-bounceable value
//!      transfer). On the user wallet's first outgoing send it self-deploys (StateInit).
//!   2. **Jetton consolidation.** Once funded, the user wallet signs a TEP-74 transfer of
//!      its full USDT balance to the treasury owner, with `response_destination` = the gas
//!      station, so the leftover Toncoin returns to the station rather than stranding.
//!
//! **Idempotency (no double-send), no extra persistence — like the EVM sweep:**
//!   - the jetton move is signed at the user wallet's current `seqno` and is deterministic
//!     (query_id fixed), so a re-sign of an unconfirmed sweep is the *same* message — a
//!     stale re-broadcast the wallet silently rejects once its seqno has advanced;
//!   - the **on-chain jetton balance is the truth** — once a sweep lands, the wallet reads
//!     zero and drops out, so nothing re-sweeps.
//!
//! **Gas-station seqno (the EVM monotonic nonce counter is deleted — TON drops, not queues,
//! a future seqno).** A future seqno is rejected on TON rather than mempool-queued, so the
//! station sends **one top-up at a time**: a new top-up waits until the chain seqno advances
//! past the last one we sent. Combined with a short in-memory grace, top-ups don't pile up.
//!
//! Read-mostly and **opt-in** (`SWEEP_ENABLED`): never touches TigerBeetle (the deposit was
//! already credited; this only relocates on-chain custody), off unless the operator funds
//! the gas station and turns it on.

use std::{
	collections::HashMap,
	sync::Mutex,
	time::{Duration, Instant},
};

use evbanking_auth::ServiceTokenSource;
use evbanking_contracts::signer::v1::{ProvisionAddressRequest, SignJettonTransferRequest, SignTonTransferRequest, signer_service_client::SignerServiceClient};
use sqlx::PgPool;
use tokio::sync::OnceCell;
use tokio_util::sync::CancellationToken;
use tonic::{Request, transport::Channel};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{
	config::{TonConfig, TonSweepConfig},
	infrastructure::ton_rpc::{RpcError, TonRpc},
};

/// The reserved gas-station account id (shared with the EVM sweep): a wallet holding only
/// Toncoin, used to top up user wallets with gas.
const GAS_STATION: Uuid = Uuid::from_u128(1);

/// Headroom (nanotons) a user wallet needs beyond the jetton `msg_value` to cover its
/// one-time self-deploy + compute before a jetton send can succeed.
const GAS_HEADROOM_NANO: u64 = 50_000_000;

/// Seconds a signed external message stays valid.
const VALID_WINDOW_SECS: u64 = 300;

pub struct TonSweep {
	pool: PgPool,
	rpc: TonRpc,
	signer: SignerServiceClient<Channel>,
	service_token: Option<ServiceTokenSource>,
	usdt_master: String,
	forward_ton_amount: u64,
	msg_value: u64,
	config: TonSweepConfig,
	treasury: OnceCell<String>,
	gas_station: OnceCell<String>,
	state: Mutex<GasState>,
}

impl TonSweep {
	pub fn new(pool: PgPool, channel: Channel, service_token: Option<ServiceTokenSource>, ton: &TonConfig, config: TonSweepConfig) -> Self {
		Self {
			pool,
			rpc: TonRpc::new(ton.api_url.clone(), ton.api_key.clone()),
			signer: SignerServiceClient::new(channel),
			service_token,
			usdt_master: ton.usdt_master.clone(),
			forward_ton_amount: ton.forward_ton_amount,
			msg_value: ton.msg_value,
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
			"ton sweep: consolidating jetton deposits into the treasury"
		);
		match (self.address(&self.treasury, Uuid::nil()).await, self.address(&self.gas_station, GAS_STATION).await) {
			(Ok(treasury), Ok(gas_station)) => info!(%treasury, %gas_station, "ton sweep: fund the gas station with TON — it pays gas to sweep user USDT into the treasury"),
			_ => warn!("ton sweep: could not resolve the treasury/gas-station addresses yet (will retry each cycle)"),
		}
		loop {
			if let Err(err) = self.sweep_once().await {
				warn!("ton sweep: cycle failed, retrying next poll: {err}");
			}
			tokio::select! {
				() = shutdown.cancelled() => {
					info!("ton sweep: shutdown requested — stopping");
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
			if address.eq_ignore_ascii_case(&treasury) || address.eq_ignore_ascii_case(&gas_station) {
				continue;
			}
			if let Err(err) = self.sweep_address(user_id, &address, &treasury, &gas_station).await {
				warn!(%address, "ton sweep: address cycle failed (continuing): {err}");
			}
		}
		Ok(())
	}

	async fn sweep_address(&self, user_id: Uuid, address: &str, treasury: &str, gas_station: &str) -> Result<(), SweepError> {
		// The on-chain jetton balance is the truth — nothing worth sweeping ⇒ skip.
		let Some(jetton_wallet) = self.rpc.jetton_wallet(address, &self.usdt_master).await.map_err(read_err)? else {
			return Ok(());
		};
		if jetton_wallet.balance < self.config.min_usdt {
			// Drained (consolidation mined, or dust): stamp the credited deposits so the
			// address drops out of the scan until a NEW deposit is credited.
			self.mark_swept(user_id).await?;
			return Ok(());
		}
		let gas_needed = self.msg_value + GAS_HEADROOM_NANO;
		let ton_balance = self.rpc.balance(address).await.map_err(read_err)? as u64;
		if ton_balance < gas_needed {
			// Not enough TON to deploy + pay the jetton send — top up, sweep next cycle.
			return self.top_up_gas(address, gas_station, gas_needed).await;
		}
		self.sweep_jetton(user_id, address, &jetton_wallet.address, treasury, gas_station, jetton_wallet.balance).await
	}

	async fn top_up_gas(&self, address: &str, gas_station: &str, gas_needed: u64) -> Result<(), SweepError> {
		// Best-effort grace: skip if we topped this address up very recently (still confirming).
		if let Ok(state) = self.state.lock()
			&& let Some(at) = state.recent_topups.get(address)
			&& at.elapsed() < Duration::from_secs(self.config.topup_grace_secs)
		{
			return Ok(());
		}
		// One station send in flight at a time: a future seqno would be dropped (not queued)
		// on TON, so wait until the chain seqno advances past our last top-up before sending.
		let chain = self.rpc.seqno(gas_station).await.map_err(read_err)?;
		if let Ok(state) = self.state.lock()
			&& state.last_gas_seqno.is_some_and(|last| chain <= last)
		{
			return Ok(());
		}
		// Bring the wallet to at least `gas_needed`; never below one configured top-up.
		let drop = self.config.gas_topup_nano.max(gas_needed);
		let valid_until = (now_unix() + VALID_WINDOW_SECS) as u32;
		let boc = self.sign_native(address, drop, chain, valid_until).await?;
		if let Ok(mut state) = self.state.lock() {
			state.last_gas_seqno = Some(chain);
			state.recent_topups.insert(address.to_owned(), Instant::now());
		}
		info!(%address, drop, "ton sweep: topping up gas for a deposit wallet");
		self.broadcast(&boc, "gas", address).await;
		Ok(())
	}

	async fn sweep_jetton(&self, user_id: Uuid, address: &str, jetton_wallet: &str, treasury: &str, gas_station: &str, amount: u128) -> Result<(), SweepError> {
		// Sign at the wallet's current seqno (0 on its first send ⇒ self-deploys); a re-sign
		// of an unconfirmed sweep is the same deterministic message.
		let seqno = self.rpc.seqno(address).await.map_err(read_err)?;
		let valid_until = (now_unix() + VALID_WINDOW_SECS) as u32;
		let mut request = Request::new(SignJettonTransferRequest {
			from_user_id: user_id.to_string(), // the user wallet's owner — the signer holds its key
			network: "ton".to_owned(),
			our_jetton_wallet: jetton_wallet.to_owned(),
			to_address: treasury.to_owned(),
			amount: amount.to_string(),
			response_destination: gas_station.to_owned(), // leftover TON returns to the station
			forward_ton_amount: self.forward_ton_amount,
			msg_value: self.msg_value,
			seqno,
			valid_until,
			is_testnet: false,
			wallet_version: String::new(),
		});
		if let Some(token) = &self.service_token {
			request = token.authorize(request);
		}
		let response = self
			.signer
			.clone()
			.sign_jetton_transfer(request)
			.await
			.map_err(|s| {
				super::telemetry::note_signer_error("sweep", address, s.message());
				SweepError::Signer(format!("sweep {address}: {}", s.message()))
			})?
			.into_inner();
		self.broadcast(&response.signed_boc, "sweep", address).await;
		Ok(())
	}

	async fn sign_native(&self, to: &str, amount: u64, seqno: u64, valid_until: u32) -> Result<String, SweepError> {
		let mut request = Request::new(SignTonTransferRequest {
			from_user_id: GAS_STATION.to_string(),
			network: "ton".to_owned(),
			to_address: to.to_owned(),
			amount: amount.to_string(),
			seqno,
			valid_until,
			is_testnet: false,
			wallet_version: String::new(),
		});
		if let Some(token) = &self.service_token {
			request = token.authorize(request);
		}
		let response = self
			.signer
			.clone()
			.sign_ton_transfer(request)
			.await
			.map_err(|s| {
				super::telemetry::note_signer_error("gas top-up", "gas-station", s.message());
				SweepError::Signer(format!("gas top-up {to}: {}", s.message()))
			})?
			.into_inner();
		Ok(response.signed_boc)
	}

	/// Broadcast a signed BoC, classifying the outcome per-address (never fails the whole
	/// cycle): a transport blip retries next cycle, a toncenter rejection is a per-address
	/// warning (or a loud alert if it reads like the sender is out of funds).
	async fn broadcast(&self, boc: &str, kind: &str, address: &str) {
		match self.rpc.send_message(boc).await {
			Ok(()) => info!(kind, %address, "ton sweep: broadcast message"),
			Err(RpcError::Transport(detail)) => warn!(kind, %address, "ton sweep: transport error (retry next cycle): {detail}"),
			Err(RpcError::Rpc(msg)) if msg.to_lowercase().contains("insufficient") || msg.to_lowercase().contains("balance") =>
				error!(kind, %address, "ton sweep: SENDER MAY BE OUT OF FUNDS — fund the gas station (TON): {msg}"),
			Err(RpcError::Rpc(msg)) => warn!(kind, %address, "ton sweep: toncenter rejected the message: {msg}"),
		}
	}

	/// A system wallet's TON address, resolved once via `ProvisionAddress` (`Uuid::nil()` =
	/// treasury, [`GAS_STATION`] = gas station) and cached.
	async fn address(&self, cell: &OnceCell<String>, id: Uuid) -> Result<String, SweepError> {
		cell.get_or_try_init(|| async {
			let mut request = Request::new(ProvisionAddressRequest {
				user_id: id.to_string(),
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
	/// cycle has yet observed drained — O(active deposits), not O(all addresses).
	async fn deposit_addresses(&self) -> Result<Vec<(Uuid, String)>, SweepError> {
		sqlx::query_as::<_, (Uuid, String)>(
			"SELECT DISTINCT a.user_id, a.address FROM deposits d \
			 JOIN user_deposit_addresses a ON a.user_id::text = d.party_id AND a.network = d.network \
			 WHERE d.network = 'ton' AND d.party_kind = 'user' AND d.swept_at IS NULL AND a.address_kind = 'derived'",
		)
		.fetch_all(&self.pool)
		.await
		.map_err(|e| SweepError::Db(e.to_string()))
	}

	async fn mark_swept(&self, user_id: Uuid) -> Result<(), SweepError> {
		sqlx::query("UPDATE deposits SET swept_at = now() WHERE party_kind = 'user' AND party_id = $1 AND network = 'ton' AND swept_at IS NULL")
			.bind(user_id.to_string())
			.execute(&self.pool)
			.await
			.map_err(|e| SweepError::Db(e.to_string()))?;
		Ok(())
	}
}

#[derive(Default)]
struct GasState {
	/// The seqno of our last gas-station send. A new top-up waits until the chain advances
	/// past it (TON drops, not queues, a future seqno), so only one is in flight.
	last_gas_seqno: Option<u64>,
	/// Last time a top-up was sent to an address — a best-effort grace against pile-ups.
	recent_topups: HashMap<String, Instant>,
}

fn now_unix() -> u64 {
	std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
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
