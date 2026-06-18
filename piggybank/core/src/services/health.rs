use evfund_contracts::fund::v1::{CheckRequest, CheckResponse, health_service_server::HealthService};
use tonic::{Request, Response, Status};

/// Liveness probe for the gRPC surface. Backs the `core` BFF smoke path
/// (browser → BFF → gRPC).
#[derive(Default)]
pub struct Health;

#[tonic::async_trait]
impl HealthService for Health {
	async fn check(&self, _request: Request<CheckRequest>) -> Result<Response<CheckResponse>, Status> {
		Ok(Response::new(CheckResponse { status: "ok".to_string() }))
	}
}
