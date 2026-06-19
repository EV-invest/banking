//! The async gRPC authorization layer — the choke point every service mounts.
//!
//! tonic 0.13's `Interceptor` is **synchronous** (`fn call(&mut self, Request<()>)`),
//! so it cannot await JWKS verification or an in-process authorize round-trip.
//! This is therefore a bespoke [`tower::Layer`]: it pulls the bearer token from the
//! request metadata, authenticates it asynchronously via an [`Authenticate`]
//! implementor, injects the verified [`Claims`] into the request extensions on
//! success, and short-circuits with a gRPC `UNAUTHENTICATED` response otherwise.
//!
//! Two implementors plug into the same layer: [`Verifier`](crate::verifier::Verifier)
//! (downstream, stateless JWKS verification) and
//! [`Authorizer`](crate::authorizer::Authorizer) (the hub, in-process channel to
//! the auth task). Mount it per service so genuinely public surfaces (e.g. health)
//! stay unauthenticated.

use std::{
	future::Future,
	pin::Pin,
	task::{Context, Poll},
};

use tonic::body::Body;
use tower::{Layer, Service};

use crate::{AuthError, Claims};

/// Something that can authenticate a bearer token into [`Claims`].
pub trait Authenticate: Clone + Send + Sync + 'static {
	fn authenticate(&self, token: String) -> impl Future<Output = Result<Claims, AuthError>> + Send;
}

/// A [`tower::Layer`] that authorizes inbound gRPC requests with `A`.
#[derive(Clone)]
pub struct AuthLayer<A> {
	authenticator: A,
}

impl<A> AuthLayer<A> {
	pub fn new(authenticator: A) -> Self {
		Self { authenticator }
	}
}

/// Build the authorization layer for an authenticator (a [`Verifier`] downstream,
/// or the hub's [`Authorizer`]).
///
/// [`Verifier`]: crate::verifier::Verifier
/// [`Authorizer`]: crate::authorizer::Authorizer
pub fn grpc_auth_layer<A: Authenticate>(authenticator: A) -> AuthLayer<A> {
	AuthLayer::new(authenticator)
}

impl<S, A: Clone> Layer<S> for AuthLayer<A> {
	type Service = GrpcAuth<S, A>;

	fn layer(&self, inner: S) -> Self::Service {
		GrpcAuth {
			inner,
			authenticator: self.authenticator.clone(),
		}
	}
}

/// The service produced by [`AuthLayer`].
#[derive(Clone)]
pub struct GrpcAuth<S, A> {
	inner: S,
	authenticator: A,
}

impl<S, A, B> Service<http::Request<B>> for GrpcAuth<S, A>
where
	S: Service<http::Request<B>, Response = http::Response<Body>, Error = std::convert::Infallible> + Clone + Send + 'static,
	S::Future: Send + 'static,
	A: Authenticate,
	B: Send + 'static,
{
	type Error = std::convert::Infallible;
	type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;
	type Response = http::Response<Body>;

	fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		self.inner.poll_ready(cx)
	}

	fn call(&mut self, mut req: http::Request<B>) -> Self::Future {
		// Ready-clone: call the instance that was `poll_ready`'d, keep a fresh clone
		// in `self` for the next poll.
		let clone = self.inner.clone();
		let mut inner = std::mem::replace(&mut self.inner, clone);
		let authenticator = self.authenticator.clone();

		Box::pin(async move {
			let Some(token) = bearer_token(req.headers()) else {
				return Ok(status_response(&AuthError::MissingToken));
			};
			match authenticator.authenticate(token).await {
				Ok(claims) => {
					req.extensions_mut().insert(claims);
					inner.call(req).await
				}
				Err(err) => {
					if err.is_unexpected() {
						crate::telemetry::report(&err);
					}
					Ok(status_response(&err))
				}
			}
		})
	}
}

// Preserve the wrapped service's gRPC name so the tonic router can dispatch to it.
impl<S: tonic::server::NamedService, A> tonic::server::NamedService for GrpcAuth<S, A> {
	const NAME: &'static str = S::NAME;
}

/// Read the verified [`Claims`] a mounted [`AuthLayer`] injected, from a tonic
/// request. Returns `None` on an unauthenticated path (handler shouldn't trust it).
pub fn claims_of<T>(request: &tonic::Request<T>) -> Option<&Claims> {
	request.extensions().get::<Claims>()
}

fn bearer_token(headers: &http::HeaderMap) -> Option<String> {
	let value = headers.get(http::header::AUTHORIZATION)?.to_str().ok()?;
	value.strip_prefix("Bearer ").map(str::to_owned)
}

fn status_response(err: &AuthError) -> http::Response<Body> {
	let status: tonic::Status = err.into();
	// Mirror tonic's own interceptor error path: build the gRPC status response and
	// give it an empty body.
	let (parts, ()) = status.into_http::<()>().into_parts();
	http::Response::from_parts(parts, Body::empty())
}
