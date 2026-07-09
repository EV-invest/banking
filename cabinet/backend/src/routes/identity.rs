use axum::{Json, body::Bytes, extract::State, http::HeaderMap};
use axum_extra::extract::cookie::CookieJar;
use evconcierge_contracts::concierge::v1 as cc;

use crate::{
	dto,
	error::ApiError,
	routes::{editable, parse_body, require_token, verify_csrf},
	state::AppState,
};

/// `GET /api/users` — the caller's identity + editable fields, from concierge's directory.
pub async fn get_me(State(st): State<AppState>, jar: CookieJar) -> Result<Json<dto::UserProfile>, ApiError> {
	let token = require_token(&st, &jar).await?;
	Ok(Json(st.grpc.get_me(&token).await?.into()))
}

/// `PATCH /api/users` — CSRF-checked full-replace of the 10 editable profile fields
/// (missing ⇒ empty ⇒ cleared; identity/auth fields are not editable here).
pub async fn update_profile(State(st): State<AppState>, jar: CookieJar, headers: HeaderMap, body: Bytes) -> Result<Json<dto::UserProfile>, ApiError> {
	if !verify_csrf(&st, &jar, &headers) {
		return Err(ApiError::Csrf);
	}
	let token = require_token(&st, &jar).await?;
	let v = parse_body(&body);
	let req = cc::UpdateProfileRequest {
		legal_name: editable(&v, "legal_name"),
		preferred_name: editable(&v, "preferred_name"),
		phone: editable(&v, "phone"),
		date_of_birth: editable(&v, "date_of_birth"),
		nationality: editable(&v, "nationality"),
		tax_residence: editable(&v, "tax_residence"),
		residential_address: editable(&v, "residential_address"),
		language: editable(&v, "language"),
		base_currency: editable(&v, "base_currency"),
		timezone: editable(&v, "timezone"),
	};
	Ok(Json(st.grpc.update_profile(&token, req).await?.into()))
}
