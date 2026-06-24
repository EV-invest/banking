//! The hub's inbound **driving adapter** — its data-plane gRPC service surface
//! (tonic). A closed, internal surface consumed by other services and, via the
//! `clients/cabinet/backend` BFF, by browsers. Named for the hexagonal role (the services it
//! exposes), not the wire protocol; gRPC/tonic-web is the implementation detail.
//!
//! `tonic-web` (`GrpcWebLayer` + `accept_http1`) lets browser/WASM clients reach
//! the services over gRPC-Web with no separate proxy. `TraceLayer` emits a span
//! per request through the same `tracing` subscriber (and Sentry integration).
//!
//! The auth *issuance* routes are NOT here — they are served by the auth task
//! (`evbanking_auth`) on its own address. Core authorizes each inbound request via
//! `state.authorizer` (the in-process channel to that task); the async auth layer
//! attaches at the marked point below.

use std::net::SocketAddr;

use evbanking_auth::{TokenClass, grpc_auth_layer};
use evbanking_contracts::banking::v1::{
	balance_service_server::BalanceServiceServer, funds_service_server::FundsServiceServer, health_service_server::HealthServiceServer, users_service_server::UsersServiceServer,
	wallet_service_server::WalletServiceServer,
};
use tonic::transport::Server;
use tonic_web::GrpcWebLayer;
use tower::{Layer, ServiceBuilder};
use tower_http::trace::TraceLayer;

use crate::{
	AppState,
	services::{
		context::{BalanceSvc, FundsSvc, UsersSvc, WalletSvc},
		health::Health,
	},
};

pub mod context;
pub mod health;

/// Build the core tonic server and serve it on `addr` until shutdown.
///
/// Authorization: each data service is wrapped in the async auth layer, which
/// authorizes the request in-process via `state.authorizer` (a channel round-trip
/// to the auth task — never the network) and injects the verified `Claims` into the
/// request extensions. These are user-facing services, so the layer is pinned to the
/// **client** token class — `aud=banking-core`, `typ=access` only — rejecting an
/// inter-service token at the verifier (a `TokenClass::Service` layer is reserved for
/// future inter-service surfaces). `HealthService` is left **unwrapped** so the BFF
/// liveness probe stays public.
pub async fn serve(addr: SocketAddr, state: AppState) -> Result<(), tonic::transport::Error> {
	let auth = grpc_auth_layer(state.authorizer.for_class(TokenClass::Client));
	Server::builder()
		// grpc-web rides HTTP/1.1; required for the GrpcWebLayer to translate.
		.accept_http1(true)
		.layer(ServiceBuilder::new().layer(TraceLayer::new_for_grpc()).layer(GrpcWebLayer::new()).into_inner())
		.add_service(HealthServiceServer::new(Health))
		.add_service(auth.layer(UsersServiceServer::new(UsersSvc::new(state.clone()))))
		.add_service(auth.layer(BalanceServiceServer::new(BalanceSvc::new(state.clone()))))
		.add_service(auth.layer(FundsServiceServer::new(FundsSvc::new(state.clone()))))
		.add_service(auth.layer(WalletServiceServer::new(WalletSvc::new(state))))
		.serve(addr)
		.await
}
