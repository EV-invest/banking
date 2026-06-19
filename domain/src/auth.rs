//! `auth` bounded context — identities (wasm-safe half).
//!
//! The pure, transport-free identity types shared across the platform. The
//! *server-only* token machinery — JWKS, signing, verification, the tonic layer —
//! lives in the `evbanking_auth` crate, which is wasm-unsafe and therefore must
//! NOT be a dependency of this crate. Keep this module free of crypto and I/O so
//! `domain` stays wasm-safe for service frontends.

use serde::{Deserialize, Serialize};

use crate::error::DomainError;

/// The immutable external identity asserted by the identity provider (Google's
/// `sub` claim). It is the stable natural key the hub provisions a
/// [`User`](crate::users::User) against: never reused, never changing for a person,
/// and distinct from the hub's own canonical [`UserId`](crate::users::UserId)
/// (which is what the hub's first-party JWT carries as its `sub`). The hub never
/// puts Google's `sub` into its own tokens.
///
/// Serializes transparently as the bare string so the wire/storage shape is just
/// the subject value.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AuthSubject(String);

impl AuthSubject {
	/// Parse a provider subject, rejecting an empty value. Trimmed but otherwise
	/// opaque — the IdP owns its format.
	pub fn parse(raw: &str) -> Result<Self, DomainError> {
		let trimmed = raw.trim();
		if trimmed.is_empty() {
			return Err(DomainError::Validation("auth subject must not be empty".into()));
		}
		Ok(Self(trimmed.to_owned()))
	}

	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl core::fmt::Display for AuthSubject {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		f.write_str(&self.0)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parse_trims_and_rejects_empty() {
		assert_eq!(AuthSubject::parse("  g-123 ").unwrap().as_str(), "g-123");
		assert!(AuthSubject::parse("   ").is_err());
	}

	#[test]
	fn serializes_as_bare_string() {
		let json = serde_json::to_string(&AuthSubject::parse("g-1").unwrap()).unwrap();
		assert_eq!(json, "\"g-1\"");
	}
}
