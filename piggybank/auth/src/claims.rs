use serde::{Deserialize, Serialize};

/// First-party access-token claims minted by the central auth service.
///
/// Verified locally by every service. `token_version` lets the central service
/// invalidate all of a principal's tokens (a "revoke all" bumps the stored
/// version) without any per-service revocation state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
	/// Subject — the principal id (user or service).
	pub sub: String,
	/// Issuer — the central auth service.
	pub iss: String,
	/// Audience — the service/group the token is scoped to.
	pub aud: String,
	/// Expiry (unix seconds). TTL is short (5–15 min).
	pub exp: usize,
	/// Per-principal token version for coarse "revoke all" semantics.
	#[serde(default)]
	pub token_version: u64,
}
