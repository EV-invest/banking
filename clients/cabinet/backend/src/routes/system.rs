use axum::{
	Json,
	extract::State,
	http::StatusCode,
	response::{IntoResponse, Response},
};
use serde_json::{Value, json};

use crate::{error::ApiError, state::AppState};

/// `GET /api/health` — BFF smoke path: browser → here → piggybank `HealthService.Check`.
pub async fn health(State(st): State<AppState>) -> Response {
	match st.grpc.check().await {
		Ok(res) => Json(json!({ "ok": true, "backend": res.status })).into_response(),
		Err(e) => (StatusCode::BAD_GATEWAY, Json(json!({ "ok": false, "error": e.message() }))).into_response(),
	}
}

/// `GET /api/mfe-registry` — serve the microfrontend registry to the browser. The host
/// resolves each `<RemoteElement>` against this, so remotes deploy independently.
pub async fn mfe_registry(State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
	let raw = tokio::fs::read(&st.config.mfe_registry_path)
		.await
		.map_err(|e| ApiError::Internal(format!("mfe registry unavailable: {e}")))?;
	let registry: Value = serde_json::from_slice(&raw).map_err(|e| ApiError::Internal(format!("mfe registry invalid: {e}")))?;
	Ok(Json(registry))
}
