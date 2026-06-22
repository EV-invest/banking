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
/// A read-only view of one active refresh family for the "sessions & devices" surface.
pub struct SessionView {
	pub id: String,
	pub user_agent: String,
	pub ip: String,
	pub created_at: u64,
	pub last_seen: u64,
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
	pub fn issue(&self, user_id: &str, token_version: u64, ttl_secs: u64, user_agent: String, ip: String) -> IssuedRefresh {
		let family = uuid::Uuid::new_v4().to_string();
		let secret = uuid::Uuid::new_v4().to_string();
		let now = get_current_timestamp();
		let expires_at = now + ttl_secs;
		self.families.lock().unwrap_or_else(|e| e.into_inner()).insert(
			family.clone(),
			Family {
				id: uuid::Uuid::new_v4().to_string(),
				user_id: user_id.to_owned(),
				current: secret.clone(),
				prev: None,
				token_version,
				expires_at,
				user_agent,
				ip,
				created_at: now,
				last_seen: now,
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
			let now = get_current_timestamp();
			let expires_at = now + ttl_secs;
			fam.prev = Some(std::mem::replace(&mut fam.current, new_secret.clone()));
			fam.expires_at = expires_at;
			fam.last_seen = now;
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

	/// A view of the user's active (non-expired) refresh families — one per session.
	pub fn list_for_user(&self, user_id: &str) -> Vec<SessionView> {
		let now = get_current_timestamp();
		self.families
			.lock()
			.unwrap_or_else(|e| e.into_inner())
			.values()
			.filter(|f| f.user_id == user_id && now < f.expires_at)
			.map(|f| SessionView {
				id: f.id.clone(),
				user_agent: f.user_agent.clone(),
				ip: f.ip.clone(),
				created_at: f.created_at,
				last_seen: f.last_seen,
			})
			.collect()
	}

	/// Revoke the family with this session `id`, only if it belongs to `user_id`
	/// (guards cross-user revocation). Returns whether a family was removed.
	pub fn revoke_by_id(&self, user_id: &str, id: &str) -> bool {
		let mut map = self.families.lock().unwrap_or_else(|e| e.into_inner());
		let Some(key) = map.iter().find(|(_, f)| f.id == id && f.user_id == user_id).map(|(k, _)| k.clone()) else {
			return false;
		};
		map.remove(&key).is_some()
	}

	/// The session id of the family that owns this refresh handle, if it still exists.
	pub fn family_id_of(&self, refresh_token: &str) -> Option<String> {
		let family = refresh_token.split_once('.')?.0;
		self.families.lock().unwrap_or_else(|e| e.into_inner()).get(family).map(|f| f.id.clone())
	}
}

struct Family {
	/// Stable session id, preserved across rotations (the token handle changes,
	/// this does not), so the "sessions & devices" surface can address a session.
	id: String,
	user_id: String,
	current: String,
	prev: Option<String>,
	/// The user's `token_version` at issue time, so a later "revoke all" (which
	/// bumps the authoritative version in Postgres) is detected on the next refresh.
	token_version: u64,
	expires_at: u64,
	user_agent: String,
	ip: String,
	created_at: u64,
	last_seen: u64,
}

#[cfg(test)]
mod tests {
	use super::*;

	fn issue(store: &RefreshStore, user_id: &str) -> IssuedRefresh {
		store.issue(user_id, 0, 3600, String::new(), String::new())
	}

	#[test]
	fn rotate_then_reuse_revokes_family() {
		let store = RefreshStore::new();
		let issued = issue(&store, "user-1");
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
		let a = issue(&store, "user-1");
		let b = issue(&store, "user-1");
		store.revoke_user("user-1");
		assert!(store.rotate(&a.token, 3600).is_err());
		assert!(store.rotate(&b.token, 3600).is_err());
	}

	#[test]
	fn session_id_is_stable_across_rotation() {
		let store = RefreshStore::new();
		let issued = store.issue("user-1", 0, 3600, "agent".into(), "1.2.3.4".into());
		let id = store.family_id_of(&issued.token).unwrap();
		let rotated = store.rotate(&issued.token, 3600).unwrap();
		assert_eq!(store.family_id_of(&rotated.refresh.token).as_deref(), Some(id.as_str()));
		let sessions = store.list_for_user("user-1");
		assert_eq!(sessions.len(), 1);
		assert_eq!(sessions[0].id, id);
		assert_eq!(sessions[0].user_agent, "agent");
		assert!(sessions[0].last_seen >= sessions[0].created_at);
	}

	#[test]
	fn revoke_by_id_guards_cross_user() {
		let store = RefreshStore::new();
		let mine = store.issue("user-1", 0, 3600, String::new(), String::new());
		let id = store.family_id_of(&mine.token).unwrap();
		// A different user cannot revoke it.
		assert!(!store.revoke_by_id("user-2", &id));
		assert!(store.rotate(&mine.token, 3600).is_ok());
		// The owner can; a second attempt is a no-op.
		let id = store.family_id_of(&mine.token).unwrap();
		assert!(store.revoke_by_id("user-1", &id));
		assert!(!store.revoke_by_id("user-1", &id));
		assert!(store.list_for_user("user-1").is_empty());
	}
}
