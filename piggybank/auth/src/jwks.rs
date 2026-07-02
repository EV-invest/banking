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

#[cfg(test)]
mod tests {
	use jsonwebtoken::{EncodingKey, Header, encode, get_current_timestamp};

	use super::*;

	// A throwaway Ed25519 keypair, generated with `openssl genpkey -algorithm ed25519`.
	const TEST_PEM: &str = "-----BEGIN PRIVATE KEY-----\nMC4CAQAwBQYDK2VwBCIEIKolOSMXwE+tafZkX+jkKYJbmJ066f4E12wAwTIkKps6\n-----END PRIVATE KEY-----\n";
	const TEST_JWK_X: &str = "Z6BCmq9-_wo9d7co5CDW84Wn0sAC3BA0XWK2AOstpV4";
	const TEST_KID: &str = "test-kid";

	fn cache() -> JwksCache {
		let mut cache = JwksCache::new();
		cache.insert(TEST_KID.into(), DecodingKey::from_ed_components(TEST_JWK_X).unwrap());
		cache
	}

	fn policy(types: Vec<TokenType>) -> VerifyPolicy {
		VerifyPolicy {
			issuer: "https://auth.test".into(),
			audiences: vec!["banking-core".into()],
			allowed_types: types,
		}
	}

	fn mint(claims: &Claims) -> String {
		let key = EncodingKey::from_ed_pem(TEST_PEM.as_bytes()).unwrap();
		let mut header = Header::new(Algorithm::EdDSA);
		header.kid = Some(TEST_KID.into());
		encode(&header, claims, &key).unwrap()
	}

	fn claims(typ: TokenType) -> Claims {
		Claims {
			sub: "user-123".into(),
			iss: "https://auth.test".into(),
			aud: "banking-core".into(),
			exp: get_current_timestamp() + 900,
			iat: get_current_timestamp(),
			typ,
			jti: None,
			token_version: 0,
		}
	}

	#[test]
	fn rejects_non_eddsa_header() {
		// A token signed (and thus header-stamped) with HS256 must be rejected by the
		// EdDSA pin before any key lookup, so an attacker cannot downgrade the alg.
		let mut header = Header::new(Algorithm::HS256);
		header.kid = Some(TEST_KID.into());
		let token = encode(&header, &claims(TokenType::Access), &EncodingKey::from_secret(b"attacker-secret")).unwrap();
		assert!(matches!(verify_token(&token, &cache(), &policy(vec![TokenType::Access])), Err(AuthError::InvalidToken)));
	}

	#[test]
	fn rejects_wrong_typ() {
		let token = mint(&claims(TokenType::Service));
		// A valid EdDSA service token must be rejected by an access-only policy.
		assert!(matches!(verify_token(&token, &cache(), &policy(vec![TokenType::Access])), Err(AuthError::InvalidToken)));
		// And accepted when the policy allows its typ.
		assert!(verify_token(&token, &cache(), &policy(vec![TokenType::Service])).is_ok());
	}

	#[test]
	fn rejects_missing_required_claims() {
		#[derive(serde::Serialize)]
		struct Partial {
			sub: String,
			typ: TokenType,
			// no exp/iss/aud
		}
		let key = EncodingKey::from_ed_pem(TEST_PEM.as_bytes()).unwrap();
		let mut header = Header::new(Algorithm::EdDSA);
		header.kid = Some(TEST_KID.into());
		let token = encode(
			&header,
			&Partial {
				sub: "user-123".into(),
				typ: TokenType::Access,
			},
			&key,
		)
		.unwrap();
		assert!(matches!(verify_token(&token, &cache(), &policy(vec![TokenType::Access])), Err(AuthError::InvalidToken)));
	}
}
