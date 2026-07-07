use std::{collections::HashMap, sync::Arc, time::Duration};

use evbanking_contracts::banking::v1 as bk;
use evconcierge_contracts::concierge::v1 as cc;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tonic::{Code, Status};

use crate::{
	state::Grpc,
	util::{now_secs, random_token},
};

/// The authenticated principal the cabinet surfaces to the browser (never a token).
#[derive(Clone, Default, Deserialize, Serialize)]
pub struct User {
	pub user_id: String,
	pub email: String,
	pub status: String,
	/// The platform access role (snake_case: investor/operator/admin/owner), captured at
	/// login so `/api/auth/session` can gate the admin console nav without a round trip.
	pub role: String,
}

impl From<cc::UserSummary> for User {
	fn from(u: cc::UserSummary) -> Self {
		Self {
			user_id: u.user_id,
			email: u.email,
			status: u.status,
			role: u.role,
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

	/// Open a session from a fresh concierge token pair plus the (best-effort) banking
	/// money-token pair minted at login; returns `(session_id, csrf_token, max_age_secs)`.
	/// `user_agent`/`ip` are the login-time device metadata, kept so an on-demand banking
	/// re-mint carries the same device info as the login-path mint.
	pub async fn put(&self, tokens: cc::TokenResponse, banking: Option<bk::TokenResponse>, user_agent: &str, ip: &str) -> (String, String, i64) {
		let id = random_token(32);
		let csrf = random_token(32);
		let refresh_expires_at = tokens.refresh_expires_at;
		let session = Session::from_tokens(tokens, banking, csrf.clone(), user_agent, ip);
		self.insert(&id, &session).await;
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
	/// issuer + `aud`), so a leaked identity token cannot move money. Anchored on concierge
	/// session liveness (checked first), then resolves the banking pair — minting it if a
	/// login-time mint failed (the bridge may have lagged) or rotating it if near expiry.
	/// `NotIssued` when no money token can be obtained (issuance unconfigured / user not yet
	/// mirrored); the money routes surface that as `NotConfigured` rather than ever
	/// forwarding the wrong-plane token.
	pub async fn money_token(&self, id: &str, grpc: &Grpc) -> MoneyToken {
		// Outer gate: a dead concierge session means no money token at all.
		if self.ensure_fresh(id, grpc).await.is_none() {
			return MoneyToken::NoSession;
		}
		// Single-flight on the SAME per-session lock as the concierge refresh, so concurrent
		// money requests coalesce to one mint/rotate. With the Redis backend the same
		// cross-replica lock applies too: two replicas rotating one banking family (or
		// cross-overwriting the session JSON) would strand a rotated-out token.
		let lock = self.lock_for(id).await;
		let _guard = lock.lock().await;
		let redis_lock = match self {
			Self::Redis(s) => match s.acquire_refresh_lock(id).await {
				Some(nonce) => Some(nonce),
				// Another replica holds the lock past our poll budget; serve whatever it
				// left behind rather than racing its rotation.
				None => {
					return match self.get(id).await {
						Some(s) if !s.banking_access_token.is_empty() && s.banking_access_expires_at > now_secs() + 30 => MoneyToken::Token(s.banking_access_token),
						Some(_) => MoneyToken::NotIssued,
						None => MoneyToken::NoSession,
					};
				}
			},
			Self::InProcess(_) => None,
		};
		let result = match self.get(id).await {
			Some(mut session) => match ensure_banking_token(&mut session, grpc).await {
				Some(token) => {
					self.insert(id, &session).await;
					MoneyToken::Token(token)
				}
				None => MoneyToken::NotIssued,
			},
			None => MoneyToken::NoSession,
		};
		if let (Self::Redis(s), Some(nonce)) = (self, redis_lock) {
			s.release_refresh_lock(id, &nonce).await;
		}
		result
	}

	/// The session's refresh token — proves identity to concierge's session RPCs. `None` if expired.
	pub async fn refresh_token(&self, id: &str) -> Option<String> {
		let s = self.get(id).await?;
		(s.refresh_expires_at > now_secs()).then_some(s.refresh_token)
	}

	/// Forget a session, surfacing BOTH plane families' refresh tokens so logout can revoke each
	/// upstream (concierge identity + banking money). Each is `Some` only if still within its
	/// refresh window (an expired family is already dead upstream).
	pub async fn forget(&self, id: &str) -> Option<ForgottenSession> {
		let session = match self {
			Self::InProcess(s) => s.forget(id).await,
			Self::Redis(s) => s.forget(id).await,
		}?;
		Some(ForgottenSession {
			concierge_refresh: (session.refresh_expires_at > now_secs()).then_some(session.refresh_token),
			banking_refresh: (!session.banking_refresh_token.is_empty() && session.banking_refresh_expires_at > now_secs()).then_some(session.banking_refresh_token),
		})
	}

	async fn get(&self, id: &str) -> Option<Session> {
		match self {
			Self::InProcess(s) => s.get(id).await,
			Self::Redis(s) => s.get(id).await,
		}
	}

	async fn insert(&self, id: &str, session: &Session) {
		match self {
			Self::InProcess(s) => s.insert(id.to_string(), session.clone()).await,
			Self::Redis(s) => s.insert(id, session).await,
		}
	}

	async fn lock_for(&self, id: &str) -> Arc<Mutex<()>> {
		match self {
			Self::InProcess(s) => s.lock_for(id).await,
			Self::Redis(s) => s.lock_for(id).await,
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
			Err(status) if refresh_rejected(&status) => {
				tracing::warn!(code = ?status.code(), "concierge refresh rejected; dropping the session");
				self.forget(id).await;
				None
			}
			Err(status) => {
				tracing::warn!(code = ?status.code(), detail = %status.message(), "concierge refresh failed transiently; keeping the session for retry");
				None
			}
		}
	}

	async fn forget(&self, id: &str) -> Option<Session> {
		self.locks.lock().await.remove(id);
		self.sessions.lock().await.remove(id)
	}
}

/// Whether a failed concierge refresh is a terminal auth verdict — the family is dead
/// upstream (revoked, or rotated-out reuse read as theft) — as opposed to a transport-class
/// failure (`Unavailable`/`DeadlineExceeded`/…). Only a verdict destroys the session; a
/// blip fails the request but keeps the session, so a concierge outage never logs every
/// near-expiry user out.
fn refresh_rejected(status: &Status) -> bool {
	matches!(status.code(), Code::Unauthenticated | Code::PermissionDenied)
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

	fn lock_key(id: &str) -> String {
		format!("refresh_lock:{id}")
	}

	async fn insert(&self, id: &str, session: &Session) {
		let mut conn = self.conn.clone();
		let payload = serde_json::to_string(session).expect("session serializes");
		let ttl = (session.refresh_expires_at - now_secs()).max(1);
		// Best-effort: a Redis blip drops the new session rather than the whole request — the
		// browser simply re-authenticates.
		if let Err(err) = conn.set_ex::<_, _, ()>(Self::key(id), payload, ttl as u64).await {
			tracing::warn!(%err, "session write to Redis failed; the session will not persist");
		}
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
		// The in-process mutex only serializes this replica; the Redis lock serializes
		// replicas — two presenting the same rotated-out refresh token concurrently reads
		// as theft upstream and revokes the whole family.
		let lock = self.lock_for(id).await;
		let _guard = lock.lock().await;
		match self.acquire_refresh_lock(id).await {
			Some(nonce) => {
				let refreshed = self.refresh_locked(id, grpc).await;
				self.release_refresh_lock(id, &nonce).await;
				refreshed
			}
			// Another replica held the lock past our poll budget: re-read — the winner may
			// have already refreshed. If not, fail this request; the session survives.
			None => {
				let current = self.get(id).await?;
				(current.access_expires_at > now_secs() + 30).then_some((current.user, current.csrf_token))
			}
		}
	}

	/// The refresh critical section — the caller holds both the in-process and the Redis
	/// lock. Re-checks freshness first (the lock winner on another replica may have rotated
	/// already), then rotates via concierge.
	async fn refresh_locked(&self, id: &str, grpc: &Grpc) -> Option<(User, String)> {
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
			Err(status) if refresh_rejected(&status) => {
				tracing::warn!(code = ?status.code(), "concierge refresh rejected; dropping the session");
				self.forget(id).await;
				None
			}
			Err(status) => {
				tracing::warn!(code = ?status.code(), detail = %status.message(), "concierge refresh failed transiently; keeping the session for retry");
				None
			}
		}
	}

	/// Cross-replica single-flight: `SET refresh_lock:<id> <nonce> NX PX <ttl>`, polled
	/// briefly. `Some(nonce)` when acquired (release with [`release_refresh_lock`](Self::release_refresh_lock));
	/// `None` when another replica still holds it — the caller re-reads the session instead
	/// of racing the winner's rotation. The TTL frees the lock if the winner crashes mid-refresh.
	async fn acquire_refresh_lock(&self, id: &str) -> Option<String> {
		const LOCK_TTL_MS: u64 = 10_000;
		const POLL: Duration = Duration::from_millis(100);
		const WAIT: Duration = Duration::from_secs(2);

		let nonce = random_token(16);
		let mut conn = self.conn.clone();
		let deadline = tokio::time::Instant::now() + WAIT;
		loop {
			let acquired: Result<bool, _> = redis::cmd("SET")
				.arg(Self::lock_key(id))
				.arg(&nonce)
				.arg("NX")
				.arg("PX")
				.arg(LOCK_TTL_MS)
				.query_async(&mut conn)
				.await;
			match acquired {
				Ok(true) => return Some(nonce),
				Ok(false) => {}
				Err(err) => tracing::warn!(%err, "refresh-lock acquire failed"),
			}
			if tokio::time::Instant::now() >= deadline {
				return None;
			}
			tokio::time::sleep(POLL).await;
		}
	}

	/// Release via compare-and-delete (atomic in Lua), so a slow holder whose lock already
	/// expired can never free the CURRENT winner's lock.
	async fn release_refresh_lock(&self, id: &str, nonce: &str) {
		const UNLOCK: &str = "if redis.call('GET', KEYS[1]) == ARGV[1] then return redis.call('DEL', KEYS[1]) else return 0 end";
		let mut conn = self.conn.clone();
		if let Err(err) = redis::Script::new(UNLOCK).key(Self::lock_key(id)).arg(nonce).invoke_async::<()>(&mut conn).await {
			tracing::warn!(%err, "refresh-lock release failed (the lock will expire by TTL)");
		}
	}

	async fn forget(&self, id: &str) -> Option<Session> {
		self.locks.lock().await.remove(id);
		let session = self.get(id).await;
		let mut conn = self.conn.clone();
		let _: Result<(), _> = conn.del(Self::key(id)).await;
		session
	}
}

/// The refresh tokens a forgotten session yields, so logout can revoke BOTH plane families —
/// the concierge identity family and the banking money family. Each is `Some` only if the token
/// is still within its refresh window.
pub struct ForgottenSession {
	pub concierge_refresh: Option<String>,
	pub banking_refresh: Option<String>,
}

/// The resolution of a money-plane token for a request. The money plane has its own issuer
/// and `aud=banking-core`, so the concierge identity token never authorizes it; the banking
/// pair is minted via the concierge→banking exchange seam (`IssueUserToken`).
pub enum MoneyToken {
	/// A live banking `aud=banking-core` token to forward to the money plane.
	Token(String),
	/// A live session, but no banking token could be obtained (issuance unconfigured, or
	/// the bridge hasn't mirrored the user yet); a later request re-mints.
	NotIssued,
	/// No live session (expired or gone).
	NoSession,
}
/// Apply a successful concierge refresh onto the prior session, preserving the fields that
/// the concierge rotation does not touch (the BFF-minted CSRF token and the separate banking
/// money-token pair, which rotates on its own path).
fn refreshed_session(prior: &Session, tokens: cc::TokenResponse) -> Session {
	Session {
		access_token: tokens.access_token,
		access_expires_at: tokens.access_expires_at,
		refresh_token: tokens.refresh_token,
		refresh_expires_at: tokens.refresh_expires_at,
		banking_access_token: prior.banking_access_token.clone(),
		banking_access_expires_at: prior.banking_access_expires_at,
		banking_refresh_token: prior.banking_refresh_token.clone(),
		banking_refresh_expires_at: prior.banking_refresh_expires_at,
		user: tokens.user.map(User::from).unwrap_or_default(),
		csrf_token: prior.csrf_token.clone(),
		user_agent: prior.user_agent.clone(),
		ip: prior.ip.clone(),
	}
}

#[derive(Clone, Deserialize, Serialize)]
struct Session {
	access_token: String,
	access_expires_at: i64,
	refresh_token: String,
	refresh_expires_at: i64,
	/// The SEPARATE banking-plane (`aud=banking-core`) money-token pair, minted by the
	/// concierge→banking exchange seam (`Grpc::issue_banking_token`) and rotated independently
	/// of the concierge pair (`Grpc::refresh_banking_token`). Never derived from the concierge
	/// token. Empty until first minted; re-minted on demand if a mint failed at login (the
	/// bridge may not have mirrored the user yet).
	banking_access_token: String,
	banking_access_expires_at: i64,
	banking_refresh_token: String,
	banking_refresh_expires_at: i64,
	user: User,
	csrf_token: String,
	/// Login-time device metadata, replayed on an on-demand banking re-mint so the money
	/// family carries the same device info as the login-path mint. `#[serde(default)]` so
	/// sessions stored before these fields existed still deserialize.
	#[serde(default)]
	user_agent: String,
	#[serde(default)]
	ip: String,
}
impl Session {
	fn from_tokens(tokens: cc::TokenResponse, banking: Option<bk::TokenResponse>, csrf: String, user_agent: &str, ip: &str) -> Self {
		let mut session = Self {
			access_token: tokens.access_token,
			access_expires_at: tokens.access_expires_at,
			refresh_token: tokens.refresh_token,
			refresh_expires_at: tokens.refresh_expires_at,
			banking_access_token: String::new(),
			banking_access_expires_at: 0,
			banking_refresh_token: String::new(),
			banking_refresh_expires_at: 0,
			user: tokens.user.map(User::from).unwrap_or_default(),
			csrf_token: csrf,
			user_agent: user_agent.to_string(),
			ip: ip.to_string(),
		};
		if let Some(banking) = banking {
			session.apply_banking(banking);
		}
		session
	}

	/// Store a freshly minted/rotated banking money-token pair.
	fn apply_banking(&mut self, banking: bk::TokenResponse) {
		self.banking_access_token = banking.access_token;
		self.banking_access_expires_at = banking.access_expires_at;
		self.banking_refresh_token = banking.refresh_token;
		self.banking_refresh_expires_at = banking.refresh_expires_at;
	}
}

/// Ensure the session's banking (money-plane) access token is fresh: return it if still
/// valid, rotate it via the banking refresh family if near expiry, or mint a fresh pair if
/// absent (a mint may have failed at login before the bridge mirrored the user). Mutates
/// `session` in place and returns the usable access token, or `None` if one can't be
/// obtained (issuance unconfigured, the user isn't mirrored yet, or upstream is down). The
/// caller holds the per-session single-flight lock, mirroring the concierge refresh.
async fn ensure_banking_token(session: &mut Session, grpc: &Grpc) -> Option<String> {
	if !session.banking_access_token.is_empty() && session.banking_access_expires_at > now_secs() + 30 {
		return Some(session.banking_access_token.clone());
	}
	// Prefer rotating the existing family; fall back to minting a new one only when the
	// family is dead (absent, expired, or rejected upstream). A transport-class rotation
	// failure keeps the stored pair for retry rather than minting a parallel family. The
	// concierge session liveness is the outer gate (the caller checks it first), so this
	// only runs for a live user.
	let rotated = if !session.banking_refresh_token.is_empty() && session.banking_refresh_expires_at > now_secs() {
		match grpc.refresh_banking_token(&session.banking_refresh_token).await {
			Ok(tokens) => Some(tokens),
			Err(status) if refresh_rejected(&status) => {
				tracing::warn!(code = ?status.code(), "banking refresh rejected; re-minting via the exchange seam");
				None
			}
			Err(status) => {
				tracing::warn!(code = ?status.code(), detail = %status.message(), "banking refresh failed transiently; keeping the pair for retry");
				return None;
			}
		}
	} else {
		None
	};
	let tokens = match rotated {
		Some(tokens) => tokens,
		None if !session.user.user_id.is_empty() => match grpc.issue_banking_token(&session.user.user_id, &session.user_agent, &session.ip).await {
			Ok(tokens) => tokens,
			Err(status) => {
				tracing::warn!(code = ?status.code(), detail = %status.message(), "money-plane token re-mint failed");
				return None;
			}
		},
		None => return None,
	};
	session.apply_banking(tokens);
	Some(session.banking_access_token.clone())
}

#[cfg(test)]
mod tests {
	use super::*;

	/// A lazy `Grpc` to a black-hole address: a far-future session never refreshes, so no RPC
	/// is made and the channel stays unused — the seam test exercises pure token resolution.
	fn grpc() -> Grpc {
		Grpc::connect_lazy("http://127.0.0.1:1", "http://127.0.0.1:1", "http://127.0.0.1:1", None).expect("lazy channels")
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
				role: "investor".into(),
			}),
		}
	}

	// FB-22 / BANK-ARCH-01, CROSS-1, BANK-COMM-5: the BFF holds two token pairs. The identity
	// path serves the concierge token; the money path serves the SEPARATE banking pair — and
	// NEVER the concierge token. Here issuance is unconfigured (the test `grpc` has no issuance
	// token) so the mint can't succeed: the money path resolves to `NotIssued` (the route
	// surfaces NotConfigured) rather than ever falling back to the identity token.
	#[tokio::test]
	async fn money_path_never_serves_the_concierge_identity_token() {
		let store = SessionStore::InProcess(InProcessSessionStore::new());
		let (id, _csrf, _max_age) = store.put(fresh_concierge_tokens(), None, "", "").await;
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

	// A near-expiry session whose refresh hits an unreachable concierge (a transport-class
	// error, not an auth verdict) must SURVIVE: the request fails as unauthenticated, but
	// the session stays for retry — a concierge blip must not log every near-expiry user out.
	#[tokio::test]
	async fn transport_failure_on_refresh_keeps_the_session() {
		let store = SessionStore::InProcess(InProcessSessionStore::new());
		let mut tokens = fresh_concierge_tokens();
		tokens.access_expires_at = now_secs(); // stale access forces a refresh attempt
		let (id, _csrf, _max_age) = store.put(tokens, None, "", "").await;

		assert!(store.ensure_fresh(&id, &grpc()).await.is_none(), "the refresh against the black hole must fail");
		assert_eq!(
			store.refresh_token(&id).await.as_deref(),
			Some("concierge-refresh"),
			"a transport failure must not destroy the session"
		);
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
		Grpc::connect_lazy("http://127.0.0.1:1", "http://127.0.0.1:1", "http://127.0.0.1:1", None).expect("lazy channels")
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
				role: "investor".into(),
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
		let (id, csrf, max_age) = store.put(fresh_tokens(), None, "", "").await;
		assert!(max_age > 0);

		let (user, served_csrf) = store.ensure_fresh(&id, &grpc()).await.expect("a fresh session is found in Redis");
		assert_eq!(user.user_id, "u-1");
		assert_eq!(served_csrf, csrf);

		let identity = store.access_token(&id, &grpc()).await.expect("the concierge token is served from Redis");
		assert_eq!(identity, "concierge-access");
		assert_eq!(store.refresh_token(&id).await.as_deref(), Some("concierge-refresh"));

		let forgotten = store.forget(&id).await.expect("forget yields the forgotten session");
		assert_eq!(
			forgotten.concierge_refresh.as_deref(),
			Some("concierge-refresh"),
			"forget surfaces the concierge refresh for upstream revoke"
		);
		assert!(store.ensure_fresh(&id, &grpc()).await.is_none(), "an evicted session is gone from Redis");
		assert!(store.refresh_token(&id).await.is_none());
	}

	#[tokio::test]
	async fn money_path_never_serves_the_concierge_identity_token_through_redis() {
		let Some(store) = store().await else {
			return;
		};
		let (id, _csrf, _max_age) = store.put(fresh_tokens(), None, "", "").await;
		match store.money_token(&id, &grpc()).await {
			MoneyToken::NotIssued => {}
			MoneyToken::Token(t) => panic!("money path must not reuse the identity token, got {t:?}"),
			MoneyToken::NoSession => panic!("the session is live; expected NotIssued, not NoSession"),
		}
		store.forget(&id).await;
	}
}
