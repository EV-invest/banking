//! Auth → user synchronization: the receiving end of the [`Provisioner`] channel.
//!
//! The auth task verifies a Google identity, then asks core (over the in-process
//! channel, never the network) to provision or look up the matching [`User`]. This
//! loop is the only place that translates the auth crate's primitive DTOs into
//! domain value objects and runs the aggregate's command — so `domain` never
//! depends on `evbanking_auth` and `evbanking_auth` never depends on `domain`.
//!
//! [`Provisioner`]: evbanking_auth::Provisioner

use std::sync::Arc;

use domain::{
	auth::AuthSubject,
	error::DomainError,
	users::{Email, User, UserId},
};
use evbanking_auth::{AuthError, ProvisionCommand, ProvisionRequest, ProvisionedUser};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::ports::UserRepository;

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
		ProvisionCommand::Provision {
			auth_subject,
			email,
			email_verified,
		} => {
			let subject = AuthSubject::parse(&auth_subject).map_err(invalid_identity)?;
			let email = Email::parse(&email).map_err(invalid_identity)?;
			let user = users.provision(subject, email, email_verified).await.map_err(to_auth)?;
			Ok(summary(&user))
		}
		ProvisionCommand::Lookup { user_id } => {
			let id = parse_id(&user_id)?;
			let user = users.find_by_id(id).await.map_err(to_auth)?.ok_or_else(|| AuthError::Provider("unknown user".into()))?;
			Ok(summary(&user))
		}
		ProvisionCommand::RevokeAll { user_id } => {
			let id = parse_id(&user_id)?;
			let mut user = users.find_by_id(id).await.map_err(to_auth)?.ok_or_else(|| AuthError::Provider("unknown user".into()))?;
			user.revoke_tokens();
			users.save(&mut user).await.map_err(to_auth)?;
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

fn parse_id(raw: &str) -> Result<UserId, AuthError> {
	Uuid::parse_str(raw).map(UserId::from_raw).map_err(|_| AuthError::Provider("invalid user id".into()))
}

fn invalid_identity(_: DomainError) -> AuthError {
	AuthError::Provider("invalid identity from provider".into())
}

fn to_auth(err: DomainError) -> AuthError {
	match err {
		// A control-plane failure is operational (maps to gRPC UNAVAILABLE upstream).
		DomainError::Repository(_) => AuthError::Unavailable,
		other => AuthError::Provider(other.to_string()),
	}
}
