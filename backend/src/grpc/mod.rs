//! gRPC: the driving tonic adapter. Closed, internal surface — consumed only by
//! other in-fund services. Scaffold: a single health probe; add services here
//! as the system grows.

use std::net::SocketAddr;

use tower_http::trace::TraceLayer;

use crate::grpc::{health::Health, proto::health_service_server::HealthServiceServer};

/// Code generated from `proto/fund/v1/*.proto` by `tonic-build` (see `build.rs`).
pub mod proto {
	tonic::include_proto!("fund.v1");
}

pub mod health;

/// Build the tonic server and serve it on `addr` until shutdown.
///
/// `TraceLayer::new_for_grpc()` emits a span per request, so gRPC traffic flows
/// through the same `tracing` subscriber (and thus the Sentry tracing
/// integration) as the HTTP side.
pub async fn serve(addr: SocketAddr) -> Result<(), tonic::transport::Error> {
	tonic::transport::Server::builder()
		.layer(TraceLayer::new_for_grpc())
		.add_service(HealthServiceServer::new(Health::default()))
		.serve(addr)
		.await
}
