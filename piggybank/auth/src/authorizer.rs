//! In-process authorization channel.
//!
//! `core` and the auth service run as separate tasks in one process. Rather than
//! re-implementing verification or round-tripping over the network, the auth
//! service hands `core` an [`Authorizer`] — a cloneable handle backed by an
//! in-process channel to the auth task (which owns the signing keys / JWKS).
//! `core`'s gRPC interceptor calls [`Authorizer::authorize`] per request.

use tokio::sync::{mpsc, oneshot};

use crate::{AuthError, Claims};

/// Which principal class a mounted layer accepts, so the auth task applies the
/// matching verify policy. The hub keeps the two classes cryptographically apart
/// at the verifier (distinct `aud` + `typ`), not by incidental downstream parsing:
/// user-facing data services accept only [`Client`](TokenClass::Client) tokens; a
/// genuinely inter-service surface accepts only [`Service`](TokenClass::Service).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TokenClass {
	/// A client access token (`aud=banking-core`, `typ=access`) — the user data plane.
	Client,
	/// An inter-service token (`aud=banking-services`, `typ=service`).
	Service,
}

/// A verification request sent from core's interceptor to the auth task.
pub struct AuthorizeRequest {
	/// The bearer access token taken from the gRPC request metadata.
	pub token: String,
	/// The principal class the mounting layer accepts; selects the verify policy.
	pub class: TokenClass,
	/// Where the auth task returns the verified claims (or an error).
	pub respond_to: oneshot::Sender<Result<Claims, AuthError>>,
}

/// Cloneable handle `core` holds to authorize gRPC requests in-process — no
/// network hop, no per-service key material. Bound to one [`TokenClass`] so the
/// layer it backs accepts only that principal class. Obtained from
/// [`AuthService::try_new`] (the client-class handle); derive the service-class
/// handle with [`Authorizer::for_class`].
///
/// [`AuthService::try_new`]: crate::service::AuthService::try_new
#[derive(Clone)]
pub struct Authorizer {
	tx: mpsc::Sender<AuthorizeRequest>,
	class: TokenClass,
}

impl Authorizer {
	pub(crate) fn new(tx: mpsc::Sender<AuthorizeRequest>) -> Self {
		Self { tx, class: TokenClass::Client }
	}

	/// A handle to the same auth task that authorizes against `class`'s policy. Mount
	/// the [`TokenClass::Service`] handle on inter-service surfaces only.
	pub fn for_class(&self, class: TokenClass) -> Self {
		Self { tx: self.tx.clone(), class }
	}

	/// Verify a bearer token by asking the auth task over the channel and awaiting
	/// its verdict. The auth task runs a real [`verify_token`](crate::jwks::verify_token)
	/// against the policy for this handle's [`TokenClass`] when signing is configured,
	/// and only answers [`AuthError::NotConfigured`] when no signing key is set (dev/CI).
	pub async fn authorize(&self, token: &str) -> Result<Claims, AuthError> {
		let (respond_to, response) = oneshot::channel();
		// A closed channel (send) or a dropped responder (recv) means the auth task
		// is unreachable — that is `Unavailable`, never `NotConfigured`. Only the
		// task's own verdict (the inner `?`) may report the flow as unconfigured.
		self.tx
			.send(AuthorizeRequest {
				token: token.to_owned(),
				class: self.class,
				respond_to,
			})
			.await
			.map_err(|_| AuthError::Unavailable)?;
		response.await.map_err(|_| AuthError::Unavailable)?
	}
}

impl crate::interceptor::Authenticate for Authorizer {
	async fn authenticate(&self, token: String) -> Result<Claims, AuthError> {
		self.authorize(&token).await
	}
}
