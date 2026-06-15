use axum::{Router, body::Body, http::Request, routing::get};
use sentry::integrations::tower::{NewSentryLayer, SentryHttpLayer};
use tower::ServiceBuilder;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::api::{handler, openapi::ApiDoc, state::AppState};

/// Assemble the HTTP router: routes, shared state, and cross-cutting middleware.
/// API routes are nested under `/api/v1`; Swagger UI is at `/swagger-ui` and the
/// raw spec at `/api-docs/openapi.json`.
pub fn build(state: AppState) -> Router {
	let routes = Router::new().route("/health", get(handler::health::health));

	Router::new()
		.nest("/api/v1", routes)
		.merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
		.layer(
			// ServiceBuilder so layer order matches the docs (avoids a memory leak
			// that can occur when binding Sentry layers directly on Router).
			ServiceBuilder::new()
				.layer(NewSentryLayer::<Request<Body>>::new_from_top())
				.layer(SentryHttpLayer::new().enable_transaction()),
		)
		.layer(TraceLayer::new_for_http())
		.layer(CorsLayer::permissive())
		.with_state(state)
}
