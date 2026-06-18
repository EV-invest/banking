//! The hub's inbound **driving adapter** — its data-plane gRPC service surface
//! (tonic). A closed, internal surface consumed by other services and, via the
//! `clients/core` BFF, by browsers. Named for the hexagonal role (the services it
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

use evbanking_contracts::banking::v1::{
	allocations_service_server::AllocationsServiceServer, balance_service_server::BalanceServiceServer, health_service_server::HealthServiceServer, users_service_server::UsersServiceServer,
};
use tonic::transport::Server;
use tonic_web::GrpcWebLayer;
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;

use crate::{
	AppState,
	services::{
		context::{AllocationsSvc, BalanceSvc, UsersSvc},
		health::Health,
	},
};

pub mod context;
pub mod health;

/// Build the core tonic server and serve it on `addr` until shutdown.
///
/// Authorization: requests are verified via `state.authorizer.authorize(token)`
/// (a channel round-trip to the auth task). Wire that as an async interceptor /
/// tower layer at the `.layer(...)` stack below as the auth feature lands.
pub async fn serve(addr: SocketAddr, state: AppState) -> Result<(), tonic::transport::Error> {
	Server::builder()
		// grpc-web rides HTTP/1.1; required for the GrpcWebLayer to translate.
		.accept_http1(true)
		.layer(ServiceBuilder::new().layer(TraceLayer::new_for_grpc()).layer(GrpcWebLayer::new()).into_inner())
		.add_service(HealthServiceServer::new(Health))
		.add_service(UsersServiceServer::new(UsersSvc::new(state.clone())))
		.add_service(BalanceServiceServer::new(BalanceSvc::new(state.clone())))
		.add_service(AllocationsServiceServer::new(AllocationsSvc::new(state)))
		.serve(addr)
		.await
}
