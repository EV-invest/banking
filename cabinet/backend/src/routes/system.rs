use axum::{
	Json,
	extract::State,
	http::StatusCode,
	response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{error::ApiError, state::AppState};

/// `GET /api/health` — BFF smoke path: browser → here → piggybank `HealthService.Check`.
pub async fn health(State(st): State<AppState>) -> Response {
	match st.grpc.check().await {
		Ok(res) => Json(json!({ "ok": true, "backend": res.status })).into_response(),
		Err(e) => {
			// Log-then-withhold (the `error.rs` discipline): a transport error can embed
			// addresses/dependency detail, so the browser gets a fixed generic string.
			tracing::warn!(code = ?e.code(), detail = %e.message(), "health check upstream error withheld from client");
			(StatusCode::BAD_GATEWAY, Json(json!({ "ok": false, "error": "upstream unavailable" }))).into_response()
		}
	}
}

/// A microfrontend registry entry — the strict, validated shape served to the browser.
/// `deny_unknown_fields` rejects a poisoned registry that carries extra/typo'd keys, and
/// the origin/integrity gate in [`validate`] rejects off-allow-list or hash-less remotes
/// before any entry reaches the host that injects it as first-party `<script>`.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MfeEntry {
	pub name: String,
	pub tag: String,
	#[serde(rename = "scriptUrl")]
	pub script_url: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub integrity: Option<String>,
	pub kind: MfeKind,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MfeKind {
	Component,
	Page,
}

/// `GET /api/mfe-registry` — serve the microfrontend registry to the browser. The host
/// resolves each `<RemoteElement>` against this, so remotes deploy independently. The
/// payload is parsed into [`MfeEntry`] and validated against the origin allow-list before
/// it is served — a malformed or off-allow-list registry fails here, never at the browser.
pub async fn mfe_registry(State(st): State<AppState>) -> Result<Json<Vec<MfeEntry>>, ApiError> {
	let raw = tokio::fs::read(&st.config.mfe_registry_path)
		.await
		.map_err(|e| ApiError::Internal(format!("mfe registry unavailable: {e}")))?;
	let entries: Vec<MfeEntry> = serde_json::from_slice(&raw).map_err(|e| ApiError::Internal(format!("mfe registry invalid: {e}")))?;
	validate(&entries, &st.config.mfe_allowed_origins).map_err(ApiError::Internal)?;
	Ok(Json(entries))
}

/// Reject any entry whose bundle origin is not same-origin (relative) or on the allow-list,
/// and require an SRI hash for cross-origin bundles (delivered atomically with the URL).
fn validate(entries: &[MfeEntry], allowed_origins: &[String]) -> Result<(), String> {
	for (i, entry) in entries.iter().enumerate() {
		match origin_of(&entry.script_url) {
			// Relative URL ⇒ same-origin (the cabinet itself), already constrained by 'self'.
			None => {}
			Some(origin) => {
				if !allowed_origins.iter().any(|o| o == &origin) {
					return Err(format!("mfe registry entry {i} ({}): origin {origin} not on the allow-list", entry.name));
				}
				if entry.integrity.as_deref().is_none_or(|h| !h.starts_with("sha")) {
					return Err(format!("mfe registry entry {i} ({}): cross-origin bundle requires an SRI integrity hash", entry.name));
				}
			}
		}
	}
	Ok(())
}

/// The `scheme://host[:port]` origin of an absolute http(s) URL, or `None` for a relative
/// (same-origin) URL. Returns an error string only for an absolute URL we refuse to parse.
fn origin_of(script_url: &str) -> Option<String> {
	let (scheme, rest) = script_url.split_once("://")?; // relative URL → same-origin → None.
	let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
	Some(format!("{scheme}://{authority}"))
}

#[cfg(test)]
mod tests {
	use super::*;

	fn entry(script_url: &str, integrity: Option<&str>) -> MfeEntry {
		MfeEntry {
			name: "x".into(),
			tag: "mfe-x".into(),
			script_url: script_url.into(),
			integrity: integrity.map(str::to_string),
			kind: MfeKind::Component,
		}
	}

	#[test]
	fn same_origin_relative_bundle_passes_without_integrity() {
		let entries = [entry("/mfe/x.js", None)];
		assert!(validate(&entries, &[]).is_ok());
	}

	#[test]
	fn off_allow_list_origin_is_rejected() {
		let entries = [entry("https://evil.example/x.js", Some("sha384-abc"))];
		let err = validate(&entries, &["https://cdn.trusted.example".into()]).unwrap_err();
		assert!(err.contains("not on the allow-list"), "{err}");
	}

	#[test]
	fn allow_listed_origin_without_integrity_is_rejected() {
		let entries = [entry("https://cdn.trusted.example/x.js", None)];
		let err = validate(&entries, &["https://cdn.trusted.example".into()]).unwrap_err();
		assert!(err.contains("SRI integrity hash"), "{err}");
	}

	#[test]
	fn allow_listed_origin_with_integrity_passes() {
		let entries = [entry("https://cdn.trusted.example/x.js", Some("sha384-abc"))];
		assert!(validate(&entries, &["https://cdn.trusted.example".into()]).is_ok());
	}

	#[test]
	fn malformed_registry_fails_to_deserialize() {
		// Unknown field (deny_unknown_fields) and missing required field both fail parsing.
		assert!(serde_json::from_str::<Vec<MfeEntry>>(r#"[{"name":"x","tag":"t","scriptUrl":"/x.js","kind":"page","evil":1}]"#).is_err());
		assert!(serde_json::from_str::<Vec<MfeEntry>>(r#"[{"name":"x","tag":"t","kind":"page"}]"#).is_err());
	}

	#[test]
	fn origin_parsing_handles_ports_and_paths() {
		assert_eq!(origin_of("https://cdn.example:8443/a/b.js?v=1"), Some("https://cdn.example:8443".into()));
		assert_eq!(origin_of("/mfe/x.js"), None);
		assert_eq!(origin_of("mfe/x.js"), None);
	}
}
