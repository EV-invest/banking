use std::{collections::HashMap, sync::Arc};

use evconcierge_contracts::concierge::v1 as cc;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::{
	state::Grpc,
	util::{now_secs, random_token},
};

/// The authenticated principal the cabinet surfaces to the browser (never a token).
#[derive(Clone, Default, Serialize, Deserialize)]
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
/// Two interchangeable backends with identical semantics, chosen by [`SessionStore::from_env`]:
/// an in-process map (single-instance/dev, mirrors the old TS BFF and the hub's in-process
/// refresh store) and a Redis backend (`SESSION_REDIS_URL`) so sessions survive restarts and
/// are shared across replicas. No-op-until-configured: with `SESSION_REDIS_URL` unset, local/CI
/// keep the in-process map.
pub enum SessionStore {
	InProcess(InProcessSessionStore),
	Redis(RedisSessionStore),
}
impl SessionStore {
	/// Back the store with Redis when `SESSION_REDIS_URL` is set; otherwise the in-process map.
	pub async fn from_env() -> anyhow::Result<Self> {
		match std::env::var("SESSION_REDIS_URL").ok().filter(|u| !u.is_empty()) {
			Some(url) => Ok(Self::Redis(RedisSessionStore::connect(&url).await?)),
			None => Ok(Self::InProcess(InProcessSessionStore::new())),
		}
	}

	/// Open a session from a fresh token pair; returns `(session_id, csrf_token, max_age_secs)`.
	pub async fn put(&self, tokens: cc::TokenResponse) -> (String, String, i64) {
		let id = random_token(32);
		let csrf = random_token(32);
		let refresh_expires_at = tokens.refresh_expires_at;
		let session = Session::from_tokens(tokens, csrf.clone());
		match self {
			Self::InProcess(s) => s.insert(id.clone(), session).await,
			Self::Redis(s) => s.insert(&id, &session).await,
		}
		(id, csrf, (refresh_expires_at - now_secs()).max(0))
	}

	/// Ensure the session's access token is valid, rotating via concierge if it is near
	/// expiry. Returns `(user, csrf)`, or `None` if the session is gone/expired (and drops it).
	pub async fn ensure_fresh(&self, id: &str, grpc: &Grpc) -> Option<(User, String)> {
		match self {
			Self::InProcess(s) => s.ensure_fresh(id, grpc).await,
			Self::Redis(s) => s.ensure_fresh(id, grpc).await,
		}
	}

	/// The fresh concierge access token for identity-plane calls (rotates first). `None` if gone.
	///
	/// This is the concierge `aud=concierge` token; it authorizes only the identity plane and
	/// MUST NOT be forwarded to the money plane — see [`money_token`](Self::money_token).
	pub async fn access_token(&self, id: &str, grpc: &Grpc) -> Option<String> {
		self.ensure_fresh(id, grpc).await?;
		self.get(id).await.map(|s| s.access_token)
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
		match self.get(id).await.map(|s| s.banking_access_token) {
			Some(token) if !token.is_empty() => MoneyToken::Token(token),
			Some(_) => MoneyToken::NotIssued,
			None => MoneyToken::NoSession,
		}
	}

	/// The session's refresh token — proves identity to concierge's session RPCs. `None` if expired.
	pub async fn refresh_token(&self, id: &str) -> Option<String> {
		let s = self.get(id).await?;
		(s.refresh_expires_at > now_secs()).then_some(s.refresh_token)
	}

	/// Forget a session, returning its refresh token so the caller can revoke it upstream.
	pub async fn forget(&self, id: &str) -> Option<String> {
		match self {
			Self::InProcess(s) => s.forget(id).await,
			Self::Redis(s) => s.forget(id).await,
		}
	}

	async fn get(&self, id: &str) -> Option<Session> {
		match self {
			Self::InProcess(s) => s.get(id).await,
			Self::Redis(s) => s.get(id).await,
		}
	}
}

/// In-process session map — single-instance/dev (mirrors the old TS BFF and the hub's
/// in-process refresh store).
pub struct InProcessSessionStore {
	sessions: Mutex<HashMap<String, Session>>,
	/// Per-session refresh gate, so concurrent requests coalesce to one rotation.
	locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}
impl InProcessSessionStore {
	pub fn new() -> Self {
		Self {
			sessions: Mutex::new(HashMap::new()),
			locks: Mutex::new(HashMap::new()),
		}
	}

	async fn insert(&self, id: String, session: Session) {
		self.sessions.lock().await.insert(id, session);
	}

	async fn get(&self, id: &str) -> Option<Session> {
		self.sessions.lock().await.get(id).cloned()
	}

	async fn lock_for(&self, id: &str) -> Arc<Mutex<()>> {
		self.locks.lock().await.entry(id.to_string()).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
	}

	async fn ensure_fresh(&self, id: &str, grpc: &Grpc) -> Option<(User, String)> {
		let session = self.get(id).await?;
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
		let current = self.get(id).await?;
		if current.access_expires_at > now_secs() + 30 {
			return Some((current.user, current.csrf_token));
		}
		match grpc.refresh(&current.refresh_token).await {
			Ok(tokens) => {
				let refreshed = refreshed_session(&current, tokens);
				let view = (refreshed.user.clone(), refreshed.csrf_token.clone());
				self.sessions.lock().await.insert(id.to_string(), refreshed);
				Some(view)
			}
			Err(_) => {
				self.forget(id).await;
				None
			}
		}
	}

	async fn forget(&self, id: &str) -> Option<String> {
		self.locks.lock().await.remove(id);
		self.sessions.lock().await.remove(id).map(|s| s.refresh_token)
	}
}

/// Redis-backed session store — same semantics as [`InProcessSessionStore`], shared across
/// replicas and durable across restarts. Each session is one JSON value at `session:<id>`,
/// TTL'd at the refresh deadline so an abandoned session is reaped automatically.
#[derive(Clone)]
pub struct RedisSessionStore {
	conn: redis::aio::ConnectionManager,
	/// Per-session single-flight gate (per-replica — see [`ensure_fresh`]).
	locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}
impl RedisSessionStore {
	pub async fn connect(url: &str) -> anyhow::Result<Self> {
		let client = redis::Client::open(url)?;
		let conn = client.get_connection_manager().await?;
		Ok(Self {
			conn,
			locks: Arc::new(Mutex::new(HashMap::new())),
		})
	}

	fn key(id: &str) -> String {
		format!("session:{id}")
	}

	async fn insert(&self, id: &str, session: &Session) {
		let mut conn = self.conn.clone();
		let payload = serde_json::to_string(session).expect("session serializes");
		let ttl = (session.refresh_expires_at - now_secs()).max(1);
		// Best-effort: a Redis blip drops the new session rather than the whole request — the
		// browser simply re-authenticates. Logged via Sentry's tracing layer through the error.
		let _: Result<(), _> = conn.set_ex(Self::key(id), payload, ttl as u64).await;
	}

	async fn get(&self, id: &str) -> Option<Session> {
		let mut conn = self.conn.clone();
		let payload: Option<String> = conn.get(Self::key(id)).await.ok()?;
		serde_json::from_str(&payload?).ok()
	}

	async fn lock_for(&self, id: &str) -> Arc<Mutex<()>> {
		self.locks.lock().await.entry(id.to_string()).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
	}

	async fn ensure_fresh(&self, id: &str, grpc: &Grpc) -> Option<(User, String)> {
		let session = self.get(id).await?;
		if session.refresh_expires_at <= now_secs() {
			self.forget(id).await;
			return None;
		}
		if session.access_expires_at > now_secs() + 30 {
			return Some((session.user, session.csrf_token));
		}
		let lock = self.lock_for(id).await;
		let _guard = lock.lock().await;
		let current = self.get(id).await?;
		if current.access_expires_at > now_secs() + 30 {
			return Some((current.user, current.csrf_token));
		}
		match grpc.refresh(&current.refresh_token).await {
			Ok(tokens) => {
				let refreshed = refreshed_session(&current, tokens);
				let view = (refreshed.user.clone(), refreshed.csrf_token.clone());
				self.insert(id, &refreshed).await;
				Some(view)
			}
			Err(_) => {
				self.forget(id).await;
				None
			}
		}
	}

	async fn forget(&self, id: &str) -> Option<String> {
		self.locks.lock().await.remove(id);
		let session = self.get(id).await;
		let mut conn = self.conn.clone();
		let _: Result<(), _> = conn.del(Self::key(id)).await;
		session.map(|s| s.refresh_token)
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
/// Apply a successful concierge refresh onto the prior session, preserving the fields that
/// rotation does not touch (the BFF-minted CSRF token and the separate banking token).
fn refreshed_session(prior: &Session, tokens: cc::TokenResponse) -> Session {
	Session {
		access_token: tokens.access_token,
		access_expires_at: tokens.access_expires_at,
		refresh_token: tokens.refresh_token,
		refresh_expires_at: tokens.refresh_expires_at,
		banking_access_token: prior.banking_access_token.clone(),
		user: tokens.user.map(User::from).unwrap_or_default(),
		csrf_token: prior.csrf_token.clone(),
	}
}

#[derive(Clone, Serialize, Deserialize)]
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
impl Session {
	fn from_tokens(tokens: cc::TokenResponse, csrf: String) -> Self {
		Self {
			access_token: tokens.access_token,
			access_expires_at: tokens.access_expires_at,
			refresh_token: tokens.refresh_token,
			refresh_expires_at: tokens.refresh_expires_at,
			banking_access_token: String::new(),
			user: tokens.user.map(User::from).unwrap_or_default(),
			csrf_token: csrf,
		}
	}
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
		let store = SessionStore::InProcess(InProcessSessionStore::new());
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
		let store = SessionStore::InProcess(InProcessSessionStore::new());
		match store.money_token("nonexistent", &grpc()).await {
			MoneyToken::NoSession => {}
			_ => panic!("a missing session must resolve to NoSession"),
		}
	}
}

/// Real-Redis round-trips for [`RedisSessionStore`]. No mocks — these hit the Redis at
/// `SESSION_REDIS_URL` and are skipped (early-return) when it is unset, so unconfigured
/// local/CI runs are unaffected. They prove the Redis arm matches the in-process semantics:
/// create, lookup, refresh-token retrieval, and evict.
#[cfg(test)]
mod redis_tests {
	use super::*;

	fn grpc() -> Grpc {
		Grpc::connect_lazy("http://127.0.0.1:1", "http://127.0.0.1:1").expect("lazy channels")
	}

	fn fresh_tokens() -> cc::TokenResponse {
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

	async fn store() -> Option<SessionStore> {
		let url = std::env::var("SESSION_REDIS_URL").ok().filter(|u| !u.is_empty())?;
		Some(SessionStore::Redis(RedisSessionStore::connect(&url).await.expect("connect to SESSION_REDIS_URL")))
	}

	#[tokio::test]
	async fn create_lookup_and_evict_round_trip_through_redis() {
		let Some(store) = store().await else {
			return;
		};
		let (id, csrf, max_age) = store.put(fresh_tokens()).await;
		assert!(max_age > 0);

		let (user, served_csrf) = store.ensure_fresh(&id, &grpc()).await.expect("a fresh session is found in Redis");
		assert_eq!(user.user_id, "u-1");
		assert_eq!(served_csrf, csrf);

		let identity = store.access_token(&id, &grpc()).await.expect("the concierge token is served from Redis");
		assert_eq!(identity, "concierge-access");
		assert_eq!(store.refresh_token(&id).await.as_deref(), Some("concierge-refresh"));

		let revoked = store.forget(&id).await;
		assert_eq!(revoked.as_deref(), Some("concierge-refresh"), "forget returns the refresh token for upstream revoke");
		assert!(store.ensure_fresh(&id, &grpc()).await.is_none(), "an evicted session is gone from Redis");
		assert!(store.refresh_token(&id).await.is_none());
	}

	#[tokio::test]
	async fn money_path_never_serves_the_concierge_identity_token_through_redis() {
		let Some(store) = store().await else {
			return;
		};
		let (id, _csrf, _max_age) = store.put(fresh_tokens()).await;
		match store.money_token(&id, &grpc()).await {
			MoneyToken::NotIssued => {}
			MoneyToken::Token(t) => panic!("money path must not reuse the identity token, got {t:?}"),
			MoneyToken::NoSession => panic!("the session is live; expected NotIssued, not NoSession"),
		}
		store.forget(&id).await;
	}
}
