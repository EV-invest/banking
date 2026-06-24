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
			banking_access_token: String::new(),
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

	/// The fresh concierge access token for identity-plane calls (rotates first). `None` if gone.
	///
	/// This is the concierge `aud=concierge` token; it authorizes only the identity plane and
	/// MUST NOT be forwarded to the money plane — see [`money_token`](Self::money_token).
	pub async fn access_token(&self, id: &str, grpc: &Grpc) -> Option<String> {
		self.ensure_fresh(id, grpc).await?;
		self.sessions.lock().await.get(id).map(|s| s.access_token.clone())
	}

	/// The fresh banking `aud=banking-core` token for money-plane calls. Distinct from the
	/// concierge identity token: the two planes are cryptographically separated (distinct
	/// issuer + `aud`), so a leaked identity token cannot move money. Returns `MoneyToken`,
	/// which is `NotIssued` until the concierge→banking token-exchange seam is built — the
	/// money routes surface that as `NotConfigured` rather than forwarding the wrong-plane
	/// token (which the money verifier would reject on issuer/audience anyway).
	pub async fn money_token(&self, id: &str, grpc: &Grpc) -> MoneyToken {
		if self.ensure_fresh(id, grpc).await.is_none() {
			return MoneyToken::NoSession;
		}
		let banking = self.sessions.lock().await.get(id).map(|s| s.banking_access_token.clone());
		match banking {
			Some(token) if !token.is_empty() => MoneyToken::Token(token),
			Some(_) => MoneyToken::NotIssued,
			None => MoneyToken::NoSession,
		}
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

/// The resolution of a money-plane token for a request. The money plane has its own issuer
/// and `aud=banking-core`, so the concierge identity token never authorizes it; until the
/// concierge→banking token-exchange seam exists, `NotIssued` is the steady state.
pub enum MoneyToken {
	/// A live banking `aud=banking-core` token to forward to the money plane.
	Token(String),
	/// A live session, but no banking token has been minted (the exchange seam is unbuilt).
	NotIssued,
	/// No live session (expired or gone).
	NoSession,
}

#[derive(Clone)]
struct Session {
	access_token: String,
	access_expires_at: i64,
	refresh_token: String,
	refresh_expires_at: i64,
	/// The separate banking-plane (`aud=banking-core`) access token for money RPCs. Empty until
	/// the concierge→banking token-exchange seam is built; never derived from the concierge token.
	/// Its own rotation/expiry machinery lands with that seam (kept off the concierge refresh path).
	banking_access_token: String,
	user: User,
	csrf_token: String,
}

#[cfg(test)]
mod tests {
	use super::*;

	/// A lazy `Grpc` to a black-hole address: a far-future session never refreshes, so no RPC
	/// is made and the channel stays unused — the seam test exercises pure token resolution.
	fn grpc() -> Grpc {
		Grpc::connect_lazy("http://127.0.0.1:1", "http://127.0.0.1:1").expect("lazy channels")
	}

	fn fresh_concierge_tokens() -> cc::TokenResponse {
		cc::TokenResponse {
			access_token: "concierge-access".into(),
			access_expires_at: now_secs() + 3600,
			refresh_token: "concierge-refresh".into(),
			refresh_expires_at: now_secs() + 86_400,
			user: Some(cc::UserSummary {
				user_id: "u-1".into(),
				email: "a@b.c".into(),
				status: "active".into(),
				token_version: 1,
			}),
		}
	}

	// FB-22 / BANK-ARCH-01, CROSS-1, BANK-COMM-5: the BFF holds two token pairs. The identity
	// path serves the concierge token; the money path serves the SEPARATE banking pair — and
	// never the concierge token. Until the concierge→banking exchange seam exists, the banking
	// pair is empty, so a money RPC resolves to `NotIssued` (the route surfaces NotConfigured)
	// rather than forwarding the identity token to the money plane.
	#[tokio::test]
	async fn money_path_never_serves_the_concierge_identity_token() {
		let store = SessionStore::new();
		let (id, _csrf, _max_age) = store.put(fresh_concierge_tokens()).await;
		let grpc = grpc();

		let identity = store.access_token(&id, &grpc).await.expect("a live session yields the concierge token");
		assert_eq!(identity, "concierge-access", "identity RPCs carry the concierge access token");

		match store.money_token(&id, &grpc).await {
			MoneyToken::NotIssued => {}
			MoneyToken::Token(t) => panic!("money path must not reuse the identity token, got {t:?}"),
			MoneyToken::NoSession => panic!("the session is live; expected NotIssued, not NoSession"),
		}
	}

	#[tokio::test]
	async fn money_token_reports_no_session_when_the_session_is_gone() {
		let store = SessionStore::new();
		match store.money_token("nonexistent", &grpc()).await {
			MoneyToken::NoSession => {}
			_ => panic!("a missing session must resolve to NoSession"),
		}
	}
}
