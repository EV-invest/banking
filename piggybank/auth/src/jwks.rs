//! JWKS public-key cache and local token verification.
//!
//! A long-lived [`JwksCache`] holds the central auth service's current signing
//! keys (keyed by `kid`), refreshed periodically and on an unknown-`kid` miss.
//! [`verify_token`] validates a token entirely against this cache — no network
//! call on the hot path.

use std::collections::HashMap;

use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode_header};

use crate::{AuthError, Claims};

/// Cached JWKS public keys, indexed by `kid`.
#[derive(Default)]
pub struct JwksCache {
	keys: HashMap<String, DecodingKey>,
}

impl JwksCache {
	pub fn new() -> Self {
		Self::default()
	}

	/// Look up a decoding key by `kid`.
	pub fn get(&self, kid: &str) -> Option<&DecodingKey> {
		self.keys.get(kid)
	}

	/// Insert/replace a decoding key. The real refresh path parses the central
	/// service's `/.well-known/jwks.json` into these entries.
	pub fn insert(&mut self, kid: String, key: DecodingKey) {
		self.keys.insert(kid, key);
	}
}

/// Verify an access token against cached JWKS keys, returning its [`Claims`].
///
/// Selects the key by the token's `kid` header, then validates the signature,
/// `exp`, `iss`, and `aud` with a pinned algorithm allowlist (EdDSA/RS256 — never
/// `none` or HS*). Stateless: no round trip, no token storage.
///
/// Scaffold: the lookup + validation shape is wired, but issuer/audience are not
/// yet configured, so this refuses with [`AuthError::NotConfigured`] rather than
/// accepting a token blindly.
pub fn verify_token(token: &str, cache: &JwksCache) -> Result<Claims, AuthError> {
	let header = decode_header(token).map_err(|_| AuthError::InvalidToken)?;
	let kid = header.kid.ok_or(AuthError::InvalidToken)?;
	let _key = cache.get(&kid).ok_or_else(|| AuthError::UnknownKid(kid.clone()))?;

	// TODO(auth feature): set_issuer/set_audience from config, decode with `_key`,
	// and return the validated claims. Pinned to an asymmetric algorithm.
	let _validation = Validation::new(Algorithm::EdDSA);
	Err(AuthError::NotConfigured)
}
