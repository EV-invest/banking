use utoipa::OpenApi;

use crate::api::handler;

/// Root of the generated OpenAPI 3.1 document — the single source of truth for
/// the HTTP contract. Paths are pulled from the `#[utoipa::path]` macros on the
/// handlers. Scaffold: only the health probe; register new handlers here.
#[derive(OpenApi)]
#[openapi(
	info(title = "EV fund API", description = "Closed finance-management API for the EV fund", version = env!("CARGO_PKG_VERSION")),
	paths(handler::health::health),
	tags((name = "health", description = "Liveness")),
)]
pub struct ApiDoc;
