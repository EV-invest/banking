use std::{collections::HashMap, sync::Arc};

use evconcierge_contracts::concierge::v1 as cc;
use tokio::sync::Mutex;

use crate::{
	state::Grpc,
	util::{now_secs, random_token},
};

/// The authenticated principal the cabinet surfaces to the browser (never a token).
#[derive(Clone, Default)]
pub struct User {
	pub user_id: String,
	pub email: String,
	pub status: String,
}

impl From<cc::UserSummary> for User {
	fn from(u: cc::UserSummary) -> Self {
		Self {
			user_id: u.user_id,
			email: u.email,
			status: u.status,
		}
	}
}

/// The server-side session store (the BFF token-handler pattern): the browser holds only
/// an opaque session id; the concierge JWTs live here and are refreshed transparently.
///
/// In-process map — single-instance/dev only (mirrors the old TS BFF and the hub's
/// in-process refresh store). Production backs this with a session store (`SESSION_REDIS_URL`).
pub struct SessionStore {
	sessions: Mutex<HashMap<String, Session>>,
	/// Per-session refresh gate, so concurrent requests coalesce to one rotation (the
	/// refresh token is single-use; without this, the first call rotates it and the rest
	/// fail with a now-stale token and drop a valid session).
	locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}
impl SessionStore {
	pub fn new() -> Self {
		Self {
			sessions: Mutex::new(HashMap::new()),
			locks: Mutex::new(HashMap::new()),
		}
	}

	/// Open a session from a fresh token pair; returns `(session_id, csrf_token, max_age_secs)`.
	pub async fn put(&self, tokens: cc::TokenResponse) -> (String, String, i64) {
		let id = random_token(32);
		let csrf = random_token(32);
		let refresh_expires_at = tokens.refresh_expires_at;
		let session = Session {
			access_token: tokens.access_token,
			access_expires_at: tokens.access_expires_at,
			refresh_token: tokens.refresh_token,
			refresh_expires_at,
			user: tokens.user.map(User::from).unwrap_or_default(),
			csrf_token: csrf.clone(),
		};
		self.sessions.lock().await.insert(id.clone(), session);
		(id, csrf, (refresh_expires_at - now_secs()).max(0))
	}

	async fn lock_for(&self, id: &str) -> Arc<Mutex<()>> {
		self.locks.lock().await.entry(id.to_string()).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
	}

	/// Ensure the session's access token is valid, rotating via concierge if it is near
	/// expiry. Returns `(user, csrf)`, or `None` if the session is gone/expired (and drops it).
	pub async fn ensure_fresh(&self, id: &str, grpc: &Grpc) -> Option<(User, String)> {
		let session = self.sessions.lock().await.get(id).cloned()?;
		if session.refresh_expires_at <= now_secs() {
			self.forget(id).await;
			return None;
		}
		if session.access_expires_at > now_secs() + 30 {
			return Some((session.user, session.csrf_token));
		}
		// Single-flight: serialize per session, then re-check (someone may have refreshed).
		let lock = self.lock_for(id).await;
		let _guard = lock.lock().await;
		let current = self.sessions.lock().await.get(id).cloned()?;
		if current.access_expires_at > now_secs() + 30 {
			return Some((current.user, current.csrf_token));
		}
		match grpc.refresh(&current.refresh_token).await {
			Ok(tokens) => {
				let mut sessions = self.sessions.lock().await;
				let s = sessions.get_mut(id)?;
				s.access_token = tokens.access_token;
				s.access_expires_at = tokens.access_expires_at;
				s.refresh_token = tokens.refresh_token;
				s.refresh_expires_at = tokens.refresh_expires_at;
				s.user = tokens.user.map(User::from).unwrap_or_default();
				Some((s.user.clone(), s.csrf_token.clone()))
			}
			Err(_) => {
				self.forget(id).await;
				None
			}
		}
	}

	/// The fresh concierge access token for BFF→plane calls (rotates first). `None` if gone.
	pub async fn access_token(&self, id: &str, grpc: &Grpc) -> Option<String> {
		self.ensure_fresh(id, grpc).await?;
		self.sessions.lock().await.get(id).map(|s| s.access_token.clone())
	}

	/// The session's refresh token — proves identity to concierge's session RPCs. `None` if expired.
	pub async fn refresh_token(&self, id: &str) -> Option<String> {
		let sessions = self.sessions.lock().await;
		let s = sessions.get(id)?;
		(s.refresh_expires_at > now_secs()).then(|| s.refresh_token.clone())
	}

	/// Forget a session, returning its refresh token so the caller can revoke it upstream.
	pub async fn forget(&self, id: &str) -> Option<String> {
		self.locks.lock().await.remove(id);
		self.sessions.lock().await.remove(id).map(|s| s.refresh_token)
	}
}

#[derive(Clone)]
struct Session {
	access_token: String,
	access_expires_at: i64,
	refresh_token: String,
	refresh_expires_at: i64,
	user: User,
	csrf_token: String,
}
