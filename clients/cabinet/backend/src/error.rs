use axum::{
	Json,
	http::StatusCode,
	response::{IntoResponse, Response},
};
use serde_json::json;
use tonic::{Code, Status};

/// The BFF's HTTP error surface. Renders as `{ "error": <message> }` with a status
/// mapped from the upstream gRPC code — matching the frontend's `httpStatusFor` /
/// `errorDetail` helpers.
pub enum ApiError {
	/// No valid session / access token (401).
	Unauthenticated,
	/// CSRF double-submit check failed (403).
	Csrf,
	/// Client input rejected before any upstream call (400).
	BadRequest(String),
	/// OAuth is not configured (no Google client id) (503).
	NotConfigured,
	/// A mutation failed upstream — surface the hub's client-safe detail.
	Grpc(Status),
	/// A read failed upstream — map the code, but surface a fixed generic message
	/// (GET reads never leak hub detail, e.g. "wallet unavailable").
	ReadFailed { code: Code, message: String },
	/// An internal failure (e.g. registry file unreadable) (502).
	Internal(String),
}

impl ApiError {
	/// Build a read failure: map the gRPC code to HTTP but use the given generic message.
	pub fn read(status: Status, message: &str) -> Self {
		ApiError::ReadFailed {
			code: status.code(),
			message: message.to_string(),
		}
	}
}

/// gRPC status code → HTTP status (mirrors `httpStatusFor` in the old TS BFF).
fn http_status_for(code: Code) -> StatusCode {
	match code {
		Code::Unauthenticated => StatusCode::UNAUTHORIZED,
		Code::PermissionDenied => StatusCode::FORBIDDEN,
		Code::InvalidArgument => StatusCode::BAD_REQUEST,
		Code::NotFound => StatusCode::NOT_FOUND,
		Code::AlreadyExists => StatusCode::CONFLICT,
		_ => StatusCode::BAD_GATEWAY,
	}
}

impl From<Status> for ApiError {
	fn from(s: Status) -> Self {
		ApiError::Grpc(s)
	}
}

impl IntoResponse for ApiError {
	fn into_response(self) -> Response {
		let (status, message) = match self {
			ApiError::Unauthenticated => (StatusCode::UNAUTHORIZED, "unauthenticated".to_string()),
			ApiError::Csrf => (StatusCode::FORBIDDEN, "csrf".to_string()),
			ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
			ApiError::NotConfigured => (StatusCode::SERVICE_UNAVAILABLE, "auth not configured".to_string()),
			ApiError::Internal(m) => (StatusCode::BAD_GATEWAY, m),
			ApiError::ReadFailed { code, message } => (http_status_for(code), message),
			ApiError::Grpc(s) => {
				// The hub's client-safe error detail (e.g. "insufficient available balance").
				let detail = if s.message().is_empty() { "request failed".to_string() } else { s.message().to_string() };
				(http_status_for(s.code()), detail)
			}
		};
		(status, Json(json!({ "error": message }))).into_response()
	}
}
