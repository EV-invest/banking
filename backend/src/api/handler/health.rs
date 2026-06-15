use axum::{Json, http::StatusCode};
use serde_json::{Value, json};

/// Liveness probe — `GET /api/v1/health`.
#[utoipa::path(get, path = "/api/v1/health", tag = "health", responses((status = 200, description = "Service is alive")))]
pub async fn health() -> (StatusCode, Json<Value>) {
	(StatusCode::OK, Json(json!({ "status": "ok" })))
}
