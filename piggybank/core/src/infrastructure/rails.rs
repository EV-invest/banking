//! Shared chain-rail plumbing — the pieces the EVM, TON and Tron rails would otherwise
//! carry in three verbatim copies.
//!
//! Each rail runs the same background shapes (deposit watcher, withdrawal watcher, treasury
//! sweep) against a different protocol. Their failure taxonomies, their wall clock, and the
//! sweep's Postgres/signer plumbing are identical — only the network name and the protocol
//! RPC differ — so they live here once, parameterised by `network: &str`, and a fix lands in
//! one place instead of three.
//!
//! What deliberately stays per-rail: the RPC clients and their `RpcError` enums (one per
//! protocol), the node error-string classifiers (`is_idempotent` matches different node
//! wording on EVM and Tron), the signing calls, and every `info!`/`warn!` the rails emit.
//! Logging stays home on purpose: `tracing` derives an event's `target` from the module it is
//! written in, and that target is both serialised into the JSON logs and what `RUST_LOG`
//! filters on. Hoisting the sweep loop's logs here would collapse
//! `piggybank_core::infrastructure::{sweep,ton_sweep,tron_sweep}` into one target and cost
//! operators the ability to filter or alert on a single rail.

use std::fmt::Display;

use evbanking_auth::ServiceTokenSource;
use evbanking_contracts::signer::v1::{ProvisionAddressRequest, signer_service_client::SignerServiceClient};
use sqlx::PgPool;
use tokio::sync::OnceCell;
use tonic::{Request, transport::Channel};
use uuid::Uuid;

/// The reserved gas-station account id, shared by every rail's sweep and distinct from the nil
/// treasury: a wallet holding only the rail's native coin (BNB/POL, Toncoin, TRX), used to top
/// up user deposit addresses with the gas/fee budget their USDT sweep burns. A separate account
/// means a separate nonce sequence, so the sweep never races the withdrawal custody path.
pub(super) const GAS_STATION: Uuid = Uuid::from_u128(1);

/// Current unix time in seconds.
///
/// These are infrastructure workers, not use cases, so they have no injected clock — the
/// figure only ever bounds a signed message's validity window or an elapsed window that the
/// chain or Postgres then stores. A clock before the epoch reads as 0.
pub(super) fn now_unix_secs() -> u64 {
	std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// [`now_unix_secs`] as a signed second count, for the call sites that hand the wall clock to
/// the fee accrual (a Postgres `bigint`) rather than to a chain.
pub(super) fn now_unix_i64() -> i64 {
	now_unix_secs() as i64
}

/// The failure taxonomy every rail's deposit and withdrawal watcher shares. No single watcher
/// constructs every variant — the TON deposit watcher, for one, never lets a per-owner RPC
/// failure end a cycle, so it never builds [`Rpc`](WatcherError::Rpc) — but the `Display`
/// prefixes are what operators read in the logs, so they stay one set across all six.
#[derive(Debug, thiserror::Error)]
pub enum WatcherError {
	#[error("rpc: {0}")]
	Rpc(String),
	#[error("decode: {0}")]
	Decode(String),
	#[error("credit: {0}")]
	Credit(String),
	#[error("db: {0}")]
	Db(String),
	#[error("custody: {0}")]
	Custody(String),
}

/// A control-plane query that failed — the watchers' `map_err` for `sqlx`.
pub(super) fn repo(err: sqlx::Error) -> WatcherError {
	WatcherError::Db(err.to_string())
}

/// The failure taxonomy every rail's treasury sweep shares.
#[derive(Debug, thiserror::Error)]
pub(super) enum SweepError {
	#[error("rpc: {0}")]
	Rpc(String),
	#[error("signer: {0}")]
	Signer(String),
	#[error("db: {0}")]
	Db(String),
	#[error("config: {0}")]
	Config(String),
}

/// A read-path RPC failure, whatever the rail's `RpcError` type is: each protocol keeps its
/// own enum, and each one's `Display` is already the operator-facing detail — so the sweeps
/// share the wrapping, not the error type.
pub(super) fn read_err(err: impl Display) -> SweepError {
	SweepError::Rpc(err.to_string())
}

/// A system wallet's on-chain address for `network`, resolved once via `ProvisionAddress`
/// (`Uuid::nil()` = treasury, [`GAS_STATION`] = gas station) and cached in `cell`. A transient
/// failure leaves the cell empty so a later cycle retries.
pub(super) async fn address(
	cell: &OnceCell<String>,
	signer: &SignerServiceClient<Channel>,
	service_token: Option<&ServiceTokenSource>,
	network: &str,
	id: Uuid,
) -> Result<String, SweepError> {
	cell.get_or_try_init(|| async {
		let mut request = Request::new(ProvisionAddressRequest {
			user_id: id.to_string(),
			network: network.to_owned(),
		});
		if let Some(token) = service_token {
			request = token.authorize(request);
		}
		let response = signer
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

/// Addresses on `network` that can still hold funds: a credited deposit exists that no sweep
/// cycle has yet observed drained. Everything else is skipped without an RPC — the scan is
/// O(active deposits), not O(all addresses ever provisioned).
pub(super) async fn deposit_addresses(pool: &PgPool, network: &str) -> Result<Vec<(Uuid, String)>, SweepError> {
	sqlx::query_as::<_, (Uuid, String)>(
		"SELECT DISTINCT a.user_id, a.address FROM deposits d \
		 JOIN user_deposit_addresses a ON a.user_id::text = d.party_id AND a.network = d.network \
		 WHERE d.network = $1 AND d.party_kind = 'user' AND d.swept_at IS NULL AND a.address_kind = 'derived'",
	)
	.bind(network)
	.fetch_all(pool)
	.await
	.map_err(|e| SweepError::Db(e.to_string()))
}

/// Stamp a user's credited deposits on `network` as swept, so the address drops out of
/// [`deposit_addresses`] until a NEW deposit is credited — the fix for the O(N)-every-cycle
/// RPC melt.
pub(super) async fn mark_swept(pool: &PgPool, network: &str, user_id: Uuid) -> Result<(), SweepError> {
	sqlx::query("UPDATE deposits SET swept_at = now() WHERE party_kind = 'user' AND party_id = $1 AND network = $2 AND swept_at IS NULL")
		.bind(user_id.to_string())
		.bind(network)
		.execute(pool)
		.await
		.map_err(|e| SweepError::Db(e.to_string()))?;
	Ok(())
}
