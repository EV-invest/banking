//! The custody/signing-service port — the narrow "broadcast this withdrawal" seam.
//!
//! Custody is a **separate trust domain** (MPC/HSM): it holds the private keys,
//! applies its own policy engine (limits, allowlists, velocity, 4-eyes), and is the
//! second gate even if the hub is compromised. The hub never signs. This port is all
//! the hub asks of it — submit an *already-reserved* withdrawal for on-chain
//! broadcast, **idempotently by `withdrawal_id`** (a retried relay delivery must not
//! double-send). A stub adapter stands in until the real custody service exists.

use async_trait::async_trait;
use domain::{
	architecture::Gateway,
	money::{Network, Usdt, WalletAddress},
};
use thiserror::Error;
use uuid::Uuid;

/// A [`Gateway`]: an external transactional system that owns its own atomicity —
/// by construction it can never enrol in a Postgres transaction.
#[async_trait]
pub trait Custody: Gateway {
	/// Submit the withdrawal's on-chain leg for signing + broadcast. MUST be
	/// idempotent by `request.withdrawal_id` so an at-least-once relay never
	/// double-spends.
	async fn broadcast(&self, request: &BroadcastRequest) -> Result<(), CustodyError>;
}
/// A request to broadcast the on-chain leg of a withdrawal. `withdrawal_id` is the
/// idempotency key the custodian MUST dedupe on.
#[derive(Debug, Clone)]
pub struct BroadcastRequest {
	pub withdrawal_id: Uuid,
	pub network: Network,
	pub address: WalletAddress,
	/// The net amount to send on-chain (gross minus the retained fee).
	pub amount: Usdt,
}

/// Failure modes the relay distinguishes — transient (retry; nothing was sent) vs a
/// policy/liquidity refusal (park for intervention; the reservation stays pending).
#[derive(Debug, Error)]
pub enum CustodyError {
	#[error("custody unavailable: {0}")]
	Unavailable(String),
	#[error("custody rejected: {0}")]
	Rejected(String),
}
