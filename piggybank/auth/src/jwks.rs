//! JWKS public-key cache and local token verification.
//!
//! A [`JwksCache`] holds the central auth service's current signing public keys
//! (by `kid`). [`verify_token`] validates a token entirely against this cache — no
//! network call on the hot path — under an explicit [`VerifyPolicy`] (issuer +
//! accepted audiences + accepted token types), with the signing algorithm pinned
//! to EdDSA (never `none`, never HS*, never a header-chosen algorithm).

use std::collections::HashMap;

use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};

use crate::{AuthError, Claims, claims::TokenType};

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

	/// Insert/replace a decoding key.
	pub fn insert(&mut self, kid: String, key: DecodingKey) {
		self.keys.insert(kid, key);
	}

	/// Replace the entire key set atomically (used by a JWKS refresh).
	pub fn replace(&mut self, keys: HashMap<String, DecodingKey>) {
		self.keys = keys;
	}

	pub fn is_empty(&self) -> bool {
		self.keys.is_empty()
	}
}

/// What a token must satisfy beyond a valid signature: the expected issuer, the
/// set of acceptable audiences, and the set of acceptable token types.
///
/// The audiences are a **set** on purpose. A downstream service pins exactly one
/// (`[svc-audience]`); the hub's own in-process verifier accepts the several
/// audiences the hub itself mints, so one verify core serves both.
#[derive(Clone, Debug)]
pub struct VerifyPolicy {
	pub issuer: String,
	pub audiences: Vec<String>,
	pub allowed_types: Vec<TokenType>,
}

/// Verify an access/service token against cached JWKS keys, returning its [`Claims`].
///
/// Selects the key by the token's `kid` header, pins the algorithm to EdDSA, then
/// validates the signature, `exp`, `iss`, and `aud`, and finally checks the `typ`
/// against the policy. Stateless: no round trip, no token storage.
pub fn verify_token(token: &str, cache: &JwksCache, policy: &VerifyPolicy) -> Result<Claims, AuthError> {
	let header = decode_header(token).map_err(|_| AuthError::InvalidToken)?;
	// Pin the algorithm from our own policy; never trust the header's choice.
	if header.alg != Algorithm::EdDSA {
		return Err(AuthError::InvalidToken);
	}
	let kid = header.kid.ok_or(AuthError::InvalidToken)?;
	let key = cache.get(&kid).ok_or(AuthError::UnknownKid(kid))?;

	let mut validation = Validation::new(Algorithm::EdDSA);
	validation.set_issuer(&[&policy.issuer]);
	validation.set_audience(&policy.audiences);
	validation.set_required_spec_claims(&["exp", "iss", "aud"]);

	let data = decode::<Claims>(token, key, &validation).map_err(|_| AuthError::InvalidToken)?;
	if !policy.allowed_types.contains(&data.claims.typ) {
		return Err(AuthError::InvalidToken);
	}
	Ok(data.claims)
}
