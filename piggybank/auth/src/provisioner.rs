//! In-process user-resolution channel (auth → core).
//!
//! The mirror image of [`Authorizer`](crate::authorizer::Authorizer): where
//! `core` asks `auth` to verify a token, `auth` asks `core` to look up the user it is
//! about to mint money-plane tokens for. `core` owns Postgres, so it is the only reader/
//! writer; `auth` owns the keys, so it is the only minter. The two never round-trip over
//! the network — this is a task-boundary channel inside the one `piggybank` process.
//!
//! Users are NOT provisioned here — that is the one-way cross-plane bridge's job
//! (concierge `CREATED`). These commands only resolve an existing mirror row (by the
//! concierge user id for issuance, or by the hub user id at refresh time) and apply the
//! durable half of a "revoke all".
//!
//! DTOs are primitive (`String`-shaped) on purpose, so this crate stays free of a
//! `domain` dependency; `core` parses them into typed ids/value objects at the edge.

use tokio::sync::{mpsc, oneshot};

use crate::AuthError;

/// What the auth service asks core to do.
#[derive(Debug)]
pub enum ProvisionCommand {
	/// Issuance: resolve the bridge-mirrored user by their CONCIERGE user id, returning
	/// the hub user id to stamp as `sub`, the effective revoke version, and a `status`
	/// that reflects the cross-plane freeze (a frozen user reads as disabled).
	ResolveForIssuance { concierge_user_id: String },
	/// Refresh-time check: fetch the current summary by hub user id (to enforce the
	/// revoke version / freeze without minting a stale token).
	Lookup { user_id: String },
	/// "Revoke all": bump the user's authoritative `token_version` in the control
	/// plane (the durable half of a logout-everywhere). Returns the updated summary.
	RevokeAll { user_id: String },
}

/// A provisioning request sent from auth to core's handler loop.
pub struct ProvisionRequest {
	pub command: ProvisionCommand,
	pub respond_to: oneshot::Sender<Result<ProvisionedUser, AuthError>>,
}

/// The snapshot core returns after provisioning/looking up a user.
#[derive(Debug, Clone)]
pub struct ProvisionedUser {
	pub user_id: String,
	pub email: String,
	pub status: String,
	pub token_version: u64,
}

impl ProvisionedUser {
	pub fn is_disabled(&self) -> bool {
		self.status == "disabled"
	}
}

/// Cloneable handle the auth service holds to provision/look up users in-process.
#[derive(Clone)]
pub struct Provisioner {
	tx: mpsc::Sender<ProvisionRequest>,
}
impl Provisioner {
	async fn send(&self, command: ProvisionCommand) -> Result<ProvisionedUser, AuthError> {
		let (respond_to, response) = oneshot::channel();
		// A closed channel or dropped responder means core's handler is gone — that
		// is `Unavailable`, never `NotConfigured`.
		self.tx.send(ProvisionRequest { command, respond_to }).await.map_err(|_| AuthError::Unavailable)?;
		response.await.map_err(|_| AuthError::Unavailable)?
	}

	/// Resolve the bridge-mirrored user by their concierge user id for token issuance.
	pub async fn resolve_for_issuance(&self, concierge_user_id: String) -> Result<ProvisionedUser, AuthError> {
		self.send(ProvisionCommand::ResolveForIssuance { concierge_user_id }).await
	}

	/// Fetch the current summary for a known hub user id.
	pub async fn lookup(&self, user_id: String) -> Result<ProvisionedUser, AuthError> {
		self.send(ProvisionCommand::Lookup { user_id }).await
	}

	/// Bump the user's authoritative `token_version` (the durable half of "revoke
	/// all"). Returns the updated summary.
	pub async fn revoke_all(&self, user_id: String) -> Result<ProvisionedUser, AuthError> {
		self.send(ProvisionCommand::RevokeAll { user_id }).await
	}
}

/// Build the provisioning channel. `core` keeps the receiver (and drains it against
/// Postgres); `auth` is handed the [`Provisioner`].
pub fn provisioner_channel() -> (Provisioner, mpsc::Receiver<ProvisionRequest>) {
	let (tx, rx) = mpsc::channel(1024);
	(Provisioner { tx }, rx)
}
