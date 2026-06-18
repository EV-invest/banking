//! The auth service — a separate application run by `core`.
//!
//! Owns the signing keys, JWKS, and refresh store; serves the **issuance** gRPC
//! routes (exchange a Google OAuth2 login for the hub's own JWT, refresh, JWKS)
//! on its own address; and answers in-process authorize requests from `core` over
//! the [`Authorizer`] channel. `core` builds it, takes the `Authorizer`, and
//! spawns [`AuthService::run`] in its own task.

use std::net::SocketAddr;

use anyhow::Context;
use evfund_contracts::fund::v1::auth_service_server::{AuthService as AuthServiceRpc, AuthServiceServer};
use tokio::sync::mpsc;
use tonic::transport::Server;

use crate::{
	AuthError,
	authorizer::{AuthorizeRequest, Authorizer},
};

/// The auth application. Holds the receive end of the [`Authorizer`] channel;
/// signing keys / JWKS / refresh store land here as the feature arrives.
pub struct AuthService {
	rx: mpsc::Receiver<AuthorizeRequest>,
}

impl AuthService {
	/// Build the service and the [`Authorizer`] handle to hand to `core`.
	pub fn new() -> (Self, Authorizer) {
		let (tx, rx) = mpsc::channel(1024);
		(Self { rx }, Authorizer::new(tx))
	}

	/// Run the auth task: serve the issuance gRPC routes ([`AuthGrpc`]) on `addr`
	/// and answer `core`'s authorize requests over the in-process channel. Whichever
	/// ends first — a server error or all [`Authorizer`]s being dropped — tears the
	/// task down.
	///
	/// Scaffold: the routes are reserved but empty (no RPCs yet) and verification is
	/// a placeholder, so the channel loop answers [`AuthError::NotConfigured`] — but
	/// the surface is real, mirroring how `core` registers its empty services.
	pub async fn run(mut self, addr: SocketAddr) -> anyhow::Result<()> {
		let issuance = Server::builder().add_service(AuthServiceServer::new(AuthGrpc)).serve(addr);
		let authorize = async {
			while let Some(request) = self.rx.recv().await {
				let _ = request.respond_to.send(Err(AuthError::NotConfigured));
			}
		};
		tokio::select! {
			result = issuance => result.context("auth issuance server error")?,
			() = authorize => {}
		}
		Ok(())
	}
}

/// gRPC issuance routes (the proto `AuthService`). Exchanging a client login for
/// the hub's JWT, refresh, and JWKS land here.
///
/// Scaffold: the proto service has no RPCs yet, so the impl is empty.
#[derive(Default)]
pub struct AuthGrpc;

impl AuthServiceRpc for AuthGrpc {}
