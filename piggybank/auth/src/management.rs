//! Token management — refresh-token rotation with reuse detection.
//!
//! Refresh tokens are opaque `"<family>.<secret>"` handles (not JWTs); the secret
//! is server-side state. Each presentation rotates the secret; presenting an
//! already-rotated secret is treated as theft and revokes the whole family
//! (OWASP refresh-rotation reuse detection).
//!
//! **Backing store.** This slice keeps the family table **in-process** (a `Mutex`
//! map) — correct and the smallest thing that works for a single-instance dev/CI
//! hub, mirroring the cabinet BFF's session store. PRODUCTION: back this with the
//! one central Redis (`REDIS_URL`) so refresh state survives restarts and is shared
//! across replicas; the public surface here does not change when that lands. A
//! per-service Redis is never introduced — verification stays stateless.

use std::{collections::HashMap, sync::Mutex};

use jsonwebtoken::get_current_timestamp;

use crate::AuthError;

/// A freshly issued refresh handle and its expiry (unix seconds).
pub struct IssuedRefresh {
	pub token: String,
	pub expires_at: u64,
}
/// The result of a successful rotation: who the family belongs to, the
/// `token_version` snapshot it was issued under, and the new handle.
pub struct RotatedRefresh {
	pub user_id: String,
	pub token_version_snapshot: u64,
	pub refresh: IssuedRefresh,
}
/// In-process refresh-token family table (see module docs for the production note).
#[derive(Default)]
pub struct RefreshStore {
	families: Mutex<HashMap<String, Family>>,
}
impl RefreshStore {
	pub fn new() -> Self {
		Self::default()
	}

	/// Open a new refresh family for a user and return its first handle.
	pub fn issue(&self, user_id: &str, token_version: u64, ttl_secs: u64) -> IssuedRefresh {
		let family = uuid::Uuid::new_v4().to_string();
		let secret = uuid::Uuid::new_v4().to_string();
		let expires_at = get_current_timestamp() + ttl_secs;
		self.families.lock().unwrap_or_else(|e| e.into_inner()).insert(
			family.clone(),
			Family {
				user_id: user_id.to_owned(),
				current: secret.clone(),
				prev: None,
				token_version,
				expires_at,
			},
		);
		IssuedRefresh {
			token: format!("{family}.{secret}"),
			expires_at,
		}
	}

	/// Rotate a presented refresh handle. Reuse of an already-rotated secret
	/// revokes the family and is reported as [`AuthError::InvalidToken`].
	pub fn rotate(&self, token: &str, ttl_secs: u64) -> Result<RotatedRefresh, AuthError> {
		let (family, secret) = token.split_once('.').ok_or(AuthError::InvalidToken)?;
		let mut map = self.families.lock().unwrap_or_else(|e| e.into_inner());
		let fam = map.get_mut(family).ok_or(AuthError::InvalidToken)?;

		if get_current_timestamp() >= fam.expires_at {
			map.remove(family);
			return Err(AuthError::InvalidToken);
		}

		if fam.current == secret {
			let new_secret = uuid::Uuid::new_v4().to_string();
			let expires_at = get_current_timestamp() + ttl_secs;
			fam.prev = Some(std::mem::replace(&mut fam.current, new_secret.clone()));
			fam.expires_at = expires_at;
			Ok(RotatedRefresh {
				user_id: fam.user_id.clone(),
				token_version_snapshot: fam.token_version,
				refresh: IssuedRefresh {
					token: format!("{family}.{new_secret}"),
					expires_at,
				},
			})
		} else if fam.prev.as_deref() == Some(secret) {
			// Reuse of a rotated-out secret — treat the family as compromised.
			map.remove(family);
			Err(AuthError::InvalidToken)
		} else {
			Err(AuthError::InvalidToken)
		}
	}

	/// The user a refresh handle belongs to, if the family still exists.
	pub fn user_of(&self, token: &str) -> Option<String> {
		let family = token.split_once('.')?.0;
		self.families.lock().unwrap_or_else(|e| e.into_inner()).get(family).map(|f| f.user_id.clone())
	}

	/// Revoke a single refresh family (one logout).
	pub fn revoke(&self, token: &str) {
		if let Some((family, _)) = token.split_once('.') {
			self.families.lock().unwrap_or_else(|e| e.into_inner()).remove(family);
		}
	}

	/// Revoke every refresh family for a user (logout everywhere / revoke all).
	pub fn revoke_user(&self, user_id: &str) {
		self.families.lock().unwrap_or_else(|e| e.into_inner()).retain(|_, f| f.user_id != user_id);
	}
}

struct Family {
	user_id: String,
	current: String,
	prev: Option<String>,
	/// The user's `token_version` at issue time, so a later "revoke all" (which
	/// bumps the authoritative version in Postgres) is detected on the next refresh.
	token_version: u64,
	expires_at: u64,
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn rotate_then_reuse_revokes_family() {
		let store = RefreshStore::new();
		let issued = store.issue("user-1", 0, 3600);
		let rotated = store.rotate(&issued.token, 3600).unwrap();
		assert_eq!(rotated.user_id, "user-1");
		// The original (now rotated-out) secret is a reuse → family revoked.
		assert!(store.rotate(&issued.token, 3600).is_err());
		// And the just-issued one is now dead too.
		assert!(store.rotate(&rotated.refresh.token, 3600).is_err());
	}

	#[test]
	fn revoke_user_drops_all_families() {
		let store = RefreshStore::new();
		let a = store.issue("user-1", 0, 3600);
		let b = store.issue("user-1", 0, 3600);
		store.revoke_user("user-1");
		assert!(store.rotate(&a.token, 3600).is_err());
		assert!(store.rotate(&b.token, 3600).is_err());
	}
}
