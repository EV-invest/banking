//! Auth → user resolution: the receiving end of the [`Provisioner`] channel.
//!
//! The money-plane auth task asks core (over the in-process channel, never the network) to
//! resolve the user it is about to mint a token for — by concierge id for issuance, by hub
//! id at refresh — plus the durable half of a "revoke all". Users are NOT provisioned here:
//! identity lives in concierge and is mirrored by the one-way bridge. This loop translates
//! the auth crate's primitive DTOs at the edge, so `domain` never depends on `evbanking_auth`
//! and `evbanking_auth` never depends on `domain`.
//!
//! [`Provisioner`]: evbanking_auth::Provisioner

use std::sync::Arc;

use domain::{
	error::DomainError,
	users::{ConciergeUserId, User, UserId},
};
use evbanking_auth::{AuthError, ProvisionCommand, ProvisionRequest, ProvisionedUser};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::ports::{IssuanceTarget, UserRepository};

/// Drain provisioning requests from the auth task until the channel closes.
pub async fn run_provisioner(mut rx: mpsc::Receiver<ProvisionRequest>, users: Arc<dyn UserRepository>) {
	while let Some(request) = rx.recv().await {
		let result = handle(users.as_ref(), request.command).await;
		// The auth task may have given up; a dropped responder is not our problem.
		let _ = request.respond_to.send(result);
	}
}

async fn handle(users: &dyn UserRepository, command: ProvisionCommand) -> Result<ProvisionedUser, AuthError> {
	match command {
		// Issuance: resolve the bridge-mirrored user by their concierge id. The money plane
		// never provisions — the user must already exist locally (bridge `CREATED`).
		ProvisionCommand::ResolveForIssuance { concierge_user_id } => {
			let concierge_id = Uuid::parse_str(&concierge_user_id)
				.map(ConciergeUserId::from_raw)
				.map_err(|_| AuthError::Provider("invalid concierge user id".into()))?;
			let target = users
				.resolve_issuance_by_concierge_id(concierge_id)
				.await
				.map_err(to_auth)?
				// Not mirrored yet: the bridge hasn't consumed this user's CREATED. Transient, not
				// an auth failure — `Unavailable` (→ UNAVAILABLE) tells the caller to retry.
				.ok_or(AuthError::Unavailable)?;
			Ok(issuance_summary(target))
		}
		// Refresh-time re-check: resolve by hub id, against the SAME bridge-mirrored slice,
		// so a concierge freeze/revoke is enforced when rotating a money-plane family.
		ProvisionCommand::Lookup { user_id } => {
			let id = parse_id(&user_id)?;
			let target = users
				.resolve_issuance_by_banking_id(id)
				.await
				.map_err(to_auth)?
				.ok_or_else(|| AuthError::Provider("unknown user".into()))?;
			Ok(issuance_summary(target))
		}
		ProvisionCommand::RevokeAll { user_id } => {
			let id = parse_id(&user_id)?;
			let user = super::users::revoke_tokens(users, id).await.map_err(to_auth)?;
			Ok(summary(&user))
		}
	}
}

fn summary(user: &User) -> ProvisionedUser {
	ProvisionedUser {
		user_id: user.id().to_string(),
		email: user.email().as_str().to_owned(),
		status: user.status().as_str().to_owned(),
		token_version: user.token_version(),
	}
}

/// Map the issuance slice to the auth summary: a disabled user (concierge freeze OR banking
/// disable) reads as `disabled` so issuance/refresh refuse a money token, and the token_version
/// is the folded revoke floor (max of the concierge and banking versions).
fn issuance_summary(target: IssuanceTarget) -> ProvisionedUser {
	ProvisionedUser {
		user_id: target.user_id.to_string(),
		email: target.email,
		status: if target.disabled { "disabled".to_owned() } else { "active".to_owned() },
		token_version: target.token_version,
	}
}

fn parse_id(raw: &str) -> Result<UserId, AuthError> {
	Uuid::parse_str(raw).map(UserId::from_raw).map_err(|_| AuthError::Provider("invalid user id".into()))
}

fn to_auth(err: DomainError) -> AuthError {
	match err {
		// A control-plane failure is operational (maps to gRPC UNAVAILABLE upstream).
		DomainError::Repository(_) => AuthError::Unavailable,
		other => AuthError::Provider(other.to_string()),
	}
}
