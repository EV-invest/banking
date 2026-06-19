//! In-process authorization channel.
//!
//! `core` and the auth service run as separate tasks in one process. Rather than
//! re-implementing verification or round-tripping over the network, the auth
//! service hands `core` an [`Authorizer`] — a cloneable handle backed by an
//! in-process channel to the auth task (which owns the signing keys / JWKS).
//! `core`'s gRPC interceptor calls [`Authorizer::authorize`] per request.

use tokio::sync::{mpsc, oneshot};

use crate::{AuthError, Claims};

/// A verification request sent from core's interceptor to the auth task.
pub struct AuthorizeRequest {
	/// The bearer access token taken from the gRPC request metadata.
	pub token: String,
	/// Where the auth task returns the verified claims (or an error).
	pub respond_to: oneshot::Sender<Result<Claims, AuthError>>,
}

/// Cloneable handle `core` holds to authorize gRPC requests in-process — no
/// network hop, no per-service key material. Obtained from [`AuthService::new`].
///
/// [`AuthService::new`]: crate::service::AuthService::new
#[derive(Clone)]
pub struct Authorizer {
	tx: mpsc::Sender<AuthorizeRequest>,
}

impl Authorizer {
	pub(crate) fn new(tx: mpsc::Sender<AuthorizeRequest>) -> Self {
		Self { tx }
	}

	/// Verify a bearer token by asking the auth task over the channel and awaiting
	/// its verdict. The channel plumbing is real; the auth task's verification is
	/// a scaffold placeholder (currently answers [`AuthError::NotConfigured`]).
	pub async fn authorize(&self, token: &str) -> Result<Claims, AuthError> {
		let (respond_to, response) = oneshot::channel();
		// A closed channel (send) or a dropped responder (recv) means the auth task
		// is unreachable — that is `Unavailable`, never `NotConfigured`. Only the
		// task's own verdict (the inner `?`) may report the flow as unconfigured.
		self.tx
			.send(AuthorizeRequest {
				token: token.to_owned(),
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
