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
//!   - the jetton move is signed at the user wallet's current `seqno`; a re-sign of an
//!     unconfirmed sweep carries a fresh validity window (not byte-identical), but the wallet's
//!     strict-seqno rule accepts at most ONE message per seqno, so once the seqno has advanced
//!     the stale twin is silently rejected — no double-sweep;
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
use evbanking_contracts::signer::v1::{SignJettonTransferRequest, SignTonTransferRequest, signer_service_client::SignerServiceClient};
use sqlx::PgPool;
use tokio::sync::OnceCell;
use tokio_util::sync::CancellationToken;
use tonic::{Request, transport::Channel};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{
	config::{TonConfig, TonSweepConfig},
	infrastructure::{
		rails::{self, GAS_STATION, SweepError, now_unix_secs, read_err},
		ton_rpc::{RpcError, TonRpc},
	},
};

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
	is_testnet: bool,
	wallet_version: String,
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
			is_testnet: ton.is_testnet,
			wallet_version: ton.wallet_version.clone(),
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
		for (user_id, address) in rails::deposit_addresses(&self.pool, "ton").await? {
			// TON base64 is case-insensitive, so a system wallet is matched case-folded.
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
			rails::mark_swept(&self.pool, "ton", user_id).await?;
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
		// on TON, so wait until the chain seqno advances past our last top-up before sending —
		// BUT only while that last send is still live. A top-up toncenter accepted (200) yet that
		// never landed (station briefly unfunded, network congestion) leaves the chain seqno
		// un-advanced; without the freshness bound the `chain <= last` gate would then early-return
		// FOREVER — every future top-up frozen, no user wallet funded, all TON deposits stranded
		// until a restart. Past `VALID_WINDOW_SECS` that send has provably expired (can never take
		// the seqno), so re-open and re-send at the same seqno; TON's strict-seqno rule still lands
		// at most one. Mirrors the withdrawal path's expired-send re-sign.
		let chain = self.rpc.seqno(gas_station).await.map_err(read_err)?;
		if let Ok(state) = self.state.lock()
			&& state.last_gas_seqno.is_some_and(|last| chain <= last)
			&& state.last_gas_at.is_some_and(|at| at.elapsed() < Duration::from_secs(VALID_WINDOW_SECS))
		{
			return Ok(());
		}
		// Bring the wallet to at least `gas_needed`; never below one configured top-up.
		let drop = self.config.gas_topup_nano.max(gas_needed);
		let valid_until = (now_unix_secs() + VALID_WINDOW_SECS) as u32;
		let boc = self.sign_native(address, drop, chain, valid_until).await?;
		// Record the top-up time BEFORE sending so a slow/failed send still gets a grace backoff.
		if let Ok(mut state) = self.state.lock() {
			state.recent_topups.insert(address.to_owned(), Instant::now());
		}
		info!(%address, drop, "ton sweep: topping up gas for a deposit wallet");
		// Advance the seqno gate ONLY on a confirmed toncenter acceptance. If it were set before the
		// send (or on a failure), a non-landing top-up — an unfunded/undeployed gas station, a
		// rejection — would never let the chain seqno advance past `last`, so the `chain <= last`
		// gate above would early-return FOREVER: every future top-up frozen, no user wallet ever
		// funded, all TON deposits stranded until a process restart. Gating on success keeps the
		// rail self-healing (mirrors the EVM sweep, which re-reads the pending nonce each cycle).
		if self.broadcast(&boc, "gas", address).await
			&& let Ok(mut state) = self.state.lock()
		{
			state.last_gas_seqno = Some(chain);
			state.last_gas_at = Some(Instant::now());
		}
		Ok(())
	}

	async fn sweep_jetton(&self, user_id: Uuid, address: &str, jetton_wallet: &str, treasury: &str, gas_station: &str, amount: u128) -> Result<(), SweepError> {
		// Sign at the wallet's current seqno (0 on its first send ⇒ self-deploys); a re-sign
		// of an unconfirmed sweep is the same deterministic message.
		let seqno = self.rpc.seqno(address).await.map_err(read_err)?;
		let valid_until = (now_unix_secs() + VALID_WINDOW_SECS) as u32;
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
			is_testnet: self.is_testnet,
			wallet_version: self.wallet_version.clone(),
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
			is_testnet: self.is_testnet,
			wallet_version: self.wallet_version.clone(),
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
	/// warning (or a loud alert if it reads like the sender is out of funds). Returns `true`
	/// only when toncenter accepted the message — the gas-station seqno gate keys off this so a
	/// failed top-up never wedges the rail.
	async fn broadcast(&self, boc: &str, kind: &str, address: &str) -> bool {
		match self.rpc.send_message(boc).await {
			Ok(()) => {
				info!(kind, %address, "ton sweep: broadcast message");
				true
			}
			Err(RpcError::Transport(detail)) => {
				warn!(kind, %address, "ton sweep: transport error (retry next cycle): {detail}");
				false
			}
			Err(RpcError::Rpc(msg)) if msg.to_lowercase().contains("insufficient") || msg.to_lowercase().contains("balance") => {
				error!(kind, %address, "ton sweep: SENDER MAY BE OUT OF FUNDS — fund the gas station (TON): {msg}");
				false
			}
			Err(RpcError::Rpc(msg)) => {
				warn!(kind, %address, "ton sweep: toncenter rejected the message: {msg}");
				false
			}
		}
	}

	async fn address(&self, cell: &OnceCell<String>, id: Uuid) -> Result<String, SweepError> {
		rails::address(cell, &self.signer, self.service_token.as_ref(), "ton", id).await
	}
}

#[derive(Default)]
struct GasState {
	/// The seqno of our last gas-station send. A new top-up waits until the chain advances
	/// past it (TON drops, not queues, a future seqno), so only one is in flight.
	last_gas_seqno: Option<u64>,
	/// When that last send went out. Bounds the wait: a top-up toncenter accepted but that
	/// never landed expires after `VALID_WINDOW_SECS`, so once this is that old the gate must
	/// re-open (re-send at the same seqno) rather than freeze forever behind a dead send.
	last_gas_at: Option<Instant>,
	/// Last time a top-up was sent to an address — a best-effort grace against pile-ups.
	recent_topups: HashMap<String, Instant>,
}
