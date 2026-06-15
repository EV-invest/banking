use tonic::{Request, Response, Status};

use crate::grpc::proto::{CheckRequest, CheckResponse, health_service_server::HealthService};

/// Liveness probe for the gRPC surface.
#[derive(Default)]
pub struct Health;

#[tonic::async_trait]
impl HealthService for Health {
	async fn check(&self, _request: Request<CheckRequest>) -> Result<Response<CheckResponse>, Status> {
		Ok(Response::new(CheckResponse { status: "ok".to_string() }))
	}
}
