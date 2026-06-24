//! The auth service — a separate application run by `core`.
//!
//! Owns the signing keys, JWKS, Google client, and refresh store; serves the
//! **issuance** gRPC routes (`Exchange`/`Refresh`/`Logout`/`Jwks`) on its own
//! address; provisions users in-process over the [`Provisioner`] channel; and
//! answers `core`'s authorize requests over the [`Authorizer`] channel. `core`
//! builds it, takes the `Authorizer`, hands it the `Provisioner`, and spawns
//! [`AuthService::run`] in its own task.
//!
//! Unconfigured (no `AUTH_SIGNING_KEY_PEM`) it runs inert: issuance and authorize
//! both answer [`AuthError::NotConfigured`], so the scaffold still boots locally.

use std::{future::Future, net::SocketAddr, sync::Arc};

use anyhow::Context;
use evbanking_contracts::banking::v1::{
	ExchangeRequest, JwksRequest, JwksResponse, ListSessionsRequest, ListSessionsResponse, LogoutRequest, LogoutResponse, RefreshRequest, RevokeSessionRequest, RevokeSessionResponse,
	Session, TokenResponse, UserSummary,
	auth_service_server::{AuthService as AuthServiceRpc, AuthServiceServer},
};
use tokio::sync::mpsc;
use tonic::{Request, Response, Status, transport::Server};

use crate::{
	AuthError, Claims,
	authorizer::{AuthorizeRequest, Authorizer, TokenClass},
	claims::TokenType,
	config::AuthConfig,
	google::GoogleOauth,
	jwks::{JwksCache, VerifyPolicy, verify_token},
	management::{IssuedRefresh, RefreshStore, SessionBounds},
	provisioner::{ProvisionedUser, Provisioner},
	signer::{Signer, load_jwks},
};

/// The auth application: the authorize-channel receiver plus the issuance engine.
pub struct AuthService {
	authorize_rx: mpsc::Receiver<AuthorizeRequest>,
	grpc: AuthGrpc,
}

impl AuthService {
	/// Build the service from config and the [`Provisioner`] handle (core keeps the
	/// receiver). Returns the [`Authorizer`] handle for core to authorize requests.
	pub fn try_new(config: AuthConfig, provisioner: Provisioner) -> anyhow::Result<(Self, Authorizer)> {
		let (tx, rx) = mpsc::channel(1024);
		let grpc = AuthGrpc::build(config, provisioner)?;
		Ok((Self { authorize_rx: rx, grpc }, Authorizer::new(tx)))
	}

	/// Run the auth task: serve issuance gRPC on `addr` and answer core's authorize
	/// requests over the in-process channel until `shutdown` fires (graceful: tonic
	/// drains in-flight issuance requests) or the channel closes.
	pub async fn run(self, addr: SocketAddr, shutdown: impl Future<Output = ()> + Send) -> anyhow::Result<()> {
		let AuthService { mut authorize_rx, grpc } = self;
		let authorizer = grpc.clone();
		let issuance = Server::builder().add_service(AuthServiceServer::new(grpc)).serve_with_shutdown(addr, shutdown);
		let authorize = async move {
			while let Some(request) = authorize_rx.recv().await {
				let result = authorizer.authorize_token(&request.token, request.class);
				let _ = request.respond_to.send(result);
			}
		};
		tokio::select! {
			result = issuance => result.context("auth issuance server error")?,
			() = authorize => {}
		}
		Ok(())
	}
}

/// The issuance engine, shared cheaply (Arc) between the gRPC server and the
/// in-process authorize loop.
#[derive(Clone)]
pub struct AuthGrpc {
	engine: Arc<AuthEngine>,
}
impl AuthGrpc {
	fn build(config: AuthConfig, provisioner: Provisioner) -> anyhow::Result<Self> {
		let (signer, keyring, jwks) = match &config.signing {
			Some(signing) => {
				let signer = Signer::try_new(signing, &config).map_err(|e| anyhow::anyhow!("auth signer init failed: {e}"))?;
				let (keyring, jwks) = load_jwks(signing).map_err(|e| anyhow::anyhow!("auth jwks load failed: {e}"))?;
				(Some(signer), keyring, jwks)
			}
			None => (None, JwksCache::new(), Vec::new()),
		};
		let google = config.google.as_ref().map(GoogleOauth::new);
		let client_policy = VerifyPolicy {
			issuer: config.issuer.clone(),
			audiences: vec![config.client_audience.clone()],
			allowed_types: vec![TokenType::Access],
		};
		let service_policy = VerifyPolicy {
			issuer: config.issuer.clone(),
			audiences: vec![config.service_audience.clone()],
			allowed_types: vec![TokenType::Service],
		};
		Ok(Self {
			engine: Arc::new(AuthEngine {
				signer,
				keyring,
				client_policy,
				service_policy,
				google,
				refresh: RefreshStore::new(),
				provisioner,
				jwks,
				session_bounds: SessionBounds {
					ttl_secs: config.refresh_ttl_secs,
					max_session_secs: config.max_session_secs,
					idle_timeout_secs: config.idle_timeout_secs,
				},
			}),
		})
	}

	/// Verify a data-plane token in-process (called from core's `Authorizer` loop)
	/// against the policy for the mounting layer's [`TokenClass`] — the same
	/// cryptographic `aud`+`typ` separation downstream services pin, so a service
	/// token is rejected at the verifier on user-facing services rather than relying
	/// on `caller_id`'s incidental UUID parse.
	fn authorize_token(&self, token: &str, class: TokenClass) -> Result<Claims, AuthError> {
		let engine = &self.engine;
		if engine.signer.is_none() || engine.keyring.is_empty() {
			return Err(AuthError::NotConfigured);
		}
		let policy = match class {
			TokenClass::Client => &engine.client_policy,
			TokenClass::Service => &engine.service_policy,
		};
		verify_token(token, &engine.keyring, policy)
	}
}

struct AuthEngine {
	signer: Option<Signer>,
	keyring: JwksCache,
	/// In-process authorize policy for user-facing data services: pins the client
	/// audience and `typ=access` only, so an inter-service token is rejected here.
	client_policy: VerifyPolicy,
	/// In-process authorize policy reserved for genuinely inter-service surfaces:
	/// pins the service audience and `typ=service` only.
	service_policy: VerifyPolicy,
	google: Option<GoogleOauth>,
	refresh: RefreshStore,
	provisioner: Provisioner,
	jwks: Vec<evbanking_contracts::banking::v1::Jwk>,
	session_bounds: SessionBounds,
}

fn token_response(access_token: String, access_exp: u64, refresh: IssuedRefresh, summary: &ProvisionedUser) -> TokenResponse {
	TokenResponse {
		access_token,
		access_expires_at: access_exp as i64,
		refresh_token: refresh.token,
		refresh_expires_at: refresh.expires_at as i64,
		user: Some(UserSummary {
			user_id: summary.user_id.clone(),
			email: summary.email.clone(),
			status: summary.status.clone(),
			token_version: summary.token_version,
		}),
	}
}

#[tonic::async_trait]
impl AuthServiceRpc for AuthGrpc {
	async fn exchange(&self, request: Request<ExchangeRequest>) -> Result<Response<TokenResponse>, Status> {
		let engine = &self.engine;
		let signer = engine.signer.as_ref().ok_or(AuthError::NotConfigured)?;
		let google = engine.google.as_ref().ok_or(AuthError::NotConfigured)?;
		let req = request.into_inner();

		let identity = google.exchange_code(&req.auth_code, &req.code_verifier, &req.redirect_uri, &req.nonce).await?;
		// Policy: an unverified Google email may sign in (the account is keyed by the
		// stable `sub`, and `email_verified` is persisted and surfaced end-to-end so
		// nothing is silently trusted), but `User::change_email` never downgrades an
		// already-verified stored email to an unverified one.
		let summary = engine.provisioner.provision(identity.subject, identity.email, identity.email_verified).await?;
		if summary.is_disabled() {
			return Err(Status::permission_denied("user is disabled"));
		}

		let (access_token, access_exp) = signer.mint_access(&summary.user_id, summary.token_version)?;
		let refresh = engine.refresh.issue(&summary.user_id, summary.token_version, engine.session_bounds, req.user_agent, req.ip);
		Ok(Response::new(token_response(access_token, access_exp, refresh, &summary)))
	}

	async fn refresh(&self, request: Request<RefreshRequest>) -> Result<Response<TokenResponse>, Status> {
		let engine = &self.engine;
		let signer = engine.signer.as_ref().ok_or(AuthError::NotConfigured)?;
		let req = request.into_inner();

		let rotated = engine.refresh.rotate(&req.refresh_token, engine.session_bounds)?;
		let summary = engine.provisioner.lookup(rotated.user_id.clone()).await?;
		if summary.is_disabled() {
			engine.refresh.revoke_user(&summary.user_id);
			return Err(Status::permission_denied("user is disabled"));
		}
		// A "revoke all" since this family was issued bumps the authoritative
		// token_version in Postgres; refuse to mint and drop the family.
		if summary.token_version > rotated.token_version_snapshot {
			engine.refresh.revoke_user(&summary.user_id);
			return Err(Status::unauthenticated("tokens revoked"));
		}

		let (access_token, access_exp) = signer.mint_access(&summary.user_id, summary.token_version)?;
		Ok(Response::new(token_response(access_token, access_exp, rotated.refresh, &summary)))
	}

	async fn logout(&self, request: Request<LogoutRequest>) -> Result<Response<LogoutResponse>, Status> {
		let engine = &self.engine;
		let req = request.into_inner();
		if req.revoke_all {
			if let Some(user_id) = engine.refresh.user_of(&req.refresh_token) {
				// Durable half: bump the authoritative token_version in the control
				// plane. Best-effort — dropping the refresh families below already ends
				// every session and access tokens expire within the short TTL, so a
				// transient control-plane blip must not fail the logout.
				if let Err(err) = engine.provisioner.revoke_all(user_id.clone()).await {
					crate::telemetry::report(&err);
				}
				engine.refresh.revoke_user(&user_id);
			}
		} else {
			engine.refresh.revoke(&req.refresh_token);
		}
		Ok(Response::new(LogoutResponse {}))
	}

	async fn list_sessions(&self, request: Request<ListSessionsRequest>) -> Result<Response<ListSessionsResponse>, Status> {
		let engine = &self.engine;
		let req = request.into_inner();
		let Some(user_id) = engine.refresh.user_of(&req.refresh_token) else {
			return Err(AuthError::InvalidToken.into());
		};
		let current_id = engine.refresh.family_id_of(&req.refresh_token);
		let sessions = engine
			.refresh
			.list_for_user(&user_id)
			.into_iter()
			.map(|s| Session {
				current: current_id.as_deref() == Some(s.id.as_str()),
				id: s.id,
				user_agent: s.user_agent,
				ip: s.ip,
				created_at: s.created_at as i64,
				last_seen: s.last_seen as i64,
			})
			.collect();
		Ok(Response::new(ListSessionsResponse { sessions }))
	}

	async fn revoke_session(&self, request: Request<RevokeSessionRequest>) -> Result<Response<RevokeSessionResponse>, Status> {
		let engine = &self.engine;
		let req = request.into_inner();
		let Some(user_id) = engine.refresh.user_of(&req.refresh_token) else {
			return Err(AuthError::InvalidToken.into());
		};
		engine.refresh.revoke_by_id(&user_id, &req.session_id);
		Ok(Response::new(RevokeSessionResponse {}))
	}

	async fn jwks(&self, _request: Request<JwksRequest>) -> Result<Response<JwksResponse>, Status> {
		Ok(Response::new(JwksResponse { keys: self.engine.jwks.clone() }))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{config::SigningConfig, provisioner::provisioner_channel};

	// Same throwaway Ed25519 keypair as the signer tests.
	const TEST_PEM: &str = "-----BEGIN PRIVATE KEY-----\nMC4CAQAwBQYDK2VwBCIEIKolOSMXwE+tafZkX+jkKYJbmJ066f4E12wAwTIkKps6\n-----END PRIVATE KEY-----\n";
	const TEST_JWK_X: &str = "Z6BCmq9-_wo9d7co5CDW84Wn0sAC3BA0XWK2AOstpV4";

	fn configured_grpc() -> AuthGrpc {
		let config = AuthConfig {
			issuer: "https://auth.test".into(),
			client_audience: "banking-core".into(),
			service_audience: "banking-services".into(),
			access_ttl_secs: 900,
			refresh_ttl_secs: 3600,
			max_session_secs: 7_776_000,
			idle_timeout_secs: 0,
			service_ttl_secs: 300,
			signing: Some(SigningConfig {
				signing_key_pem: TEST_PEM.into(),
				kid: "test-kid".into(),
				jwks_json: format!(r#"{{"keys":[{{"kty":"OKP","crv":"Ed25519","x":"{TEST_JWK_X}","kid":"test-kid","alg":"EdDSA","use":"sig"}}]}}"#),
			}),
			google: None,
		};
		let (provisioner, _rx) = provisioner_channel();
		AuthGrpc::build(config, provisioner).unwrap()
	}

	// The hub's mounted authorize path — not just the verify policy in isolation —
	// keeps the two principal classes apart: a `mint_service` token is rejected on the
	// client (user-facing) layer, and a client access token is rejected on the
	// service layer.
	#[test]
	fn authorize_token_separates_client_and_service_classes() {
		let grpc = configured_grpc();
		let signer = grpc.engine.signer.as_ref().unwrap();

		let (access, _) = signer.mint_access("00000000-0000-0000-0000-000000000001", 0).unwrap();
		let (service, _) = signer.mint_service("allocations").unwrap();

		let client_claims = grpc.authorize_token(&access, TokenClass::Client).unwrap();
		assert!(client_claims.is_access());
		assert!(grpc.authorize_token(&service, TokenClass::Client).is_err());

		let service_claims = grpc.authorize_token(&service, TokenClass::Service).unwrap();
		assert_eq!(service_claims.typ, TokenType::Service);
		assert!(grpc.authorize_token(&access, TokenClass::Service).is_err());
	}
}
