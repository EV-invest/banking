//! Integration tests for the cross-plane lifecycle bridge consumer.
//!
//! These hit a **real** Postgres (no mocks, per the project rules) and stand up a fake
//! concierge `UserEvents` server in-process (a tonic server over a localhost socket) that
//! replays a fixed list of `UserLifecycleEvent`s. They run when `DATABASE_URL` is set and
//! skip otherwise. Each test uses a fresh random `auth_subject`, so runs neither collide nor
//! require a clean database.
//!
//! Concurrency is structured (no detached `tokio::spawn`): the fake server, the consumer, and
//! the asserting driver run as branches of one `tokio::join!`; the driver cancels a shared
//! token when its assertions are done, winding the other two branches down.

use std::{future::Future, net::SocketAddr, time::Duration};

use evconcierge_contracts::concierge::v1::{
	PullUserLifecycleRequest, PullUserLifecycleResponse, UserLifecycleEvent,
	user_events_server::{UserEvents, UserEventsServer},
	user_lifecycle_event::Kind,
};
use piggybank_core::{
	infrastructure::{bridge, bridge::BridgeConsumer, db, users::PgUsers},
	ports::UserRepository,
};
use sqlx::PgPool;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tonic::{Request, Response, Status, transport::Server};

const BRIDGE_TOKEN: &str = "test-bridge-token";
/// Fixed advisory-lock key serializing the two tests' drains over the single global cursor row.
const BRIDGE_TEST_LOCK: i64 = 0x4556_4252_4944_4745;

async fn pool() -> Option<PgPool> {
	let url = std::env::var("DATABASE_URL").ok().filter(|s| !s.is_empty())?;
	let pool = db::connect(&url).await.expect("connect to Postgres");
	db::migrate(&pool).await.expect("apply migrations");
	Some(pool)
}

fn unique_subject() -> String {
	format!("itest-bridge-{}", uuid::Uuid::new_v4())
}

/// A fake concierge `UserEvents` server: serves a fixed ordered list, capped by `limit`,
/// with `position > after_position`. `position` here is the 1-based index into the list, so
/// the consumer's cursor semantics (next_position = max returned) are exercised faithfully.
/// Rejects a wrong/absent token with UNAUTHENTICATED, like the real server.
struct FakeUserEvents {
	events: Vec<UserLifecycleEvent>,
}

#[tonic::async_trait]
impl UserEvents for FakeUserEvents {
	async fn pull_user_lifecycle(&self, request: Request<PullUserLifecycleRequest>) -> Result<Response<PullUserLifecycleResponse>, Status> {
		match request.metadata().get("authorization").and_then(|v| v.to_str().ok()) {
			Some(value) if value == format!("Bearer {BRIDGE_TOKEN}") => {}
			_ => return Err(Status::unauthenticated("bad bridge token")),
		}
		let req = request.into_inner();
		let limit = req.limit.max(1) as usize;
		let mut out = Vec::new();
		let mut next = req.after_position;
		for (idx, event) in self.events.iter().enumerate() {
			let position = idx as i64 + 1;
			if position > req.after_position {
				out.push(event.clone());
				next = position;
				if out.len() >= limit {
					break;
				}
			}
		}
		Ok(Response::new(PullUserLifecycleResponse { events: out, next_position: next }))
	}
}

fn event(subject: &str, kind: Kind, sequence: u64) -> UserLifecycleEvent {
	UserLifecycleEvent {
		user_id: uuid::Uuid::new_v4().to_string(),
		kind: kind as i32,
		kyc_level: 1,
		occurred_at: 0,
		event_id: uuid::Uuid::new_v4().to_string(),
		sequence,
		auth_subject: subject.to_string(),
		email: "bridged@example.com".into(),
		email_verified: true,
		token_version: 0,
	}
}

/// A CREATED event carrying a known concierge user id — the handle the BFF later presents on
/// `IssueUserToken`, so the test can assert the bridge stores it and the resolve path finds it.
fn created_event(subject: &str, concierge_user_id: uuid::Uuid) -> UserLifecycleEvent {
	UserLifecycleEvent {
		user_id: concierge_user_id.to_string(),
		..event(subject, Kind::Created, 1)
	}
}

async fn user_id_for(pool: &PgPool, subject: &str) -> Option<uuid::Uuid> {
	sqlx::query_scalar("SELECT id FROM users WHERE auth_subject = $1")
		.bind(subject)
		.fetch_optional(pool)
		.await
		.unwrap()
}

/// Run the fake server (serving `events`) and the bridge consumer concurrently with the
/// `driver` future, all as branches of one `join!` — structured, no detached spawns. The
/// consumer drains the backlog into Postgres; `driver` waits for that, makes its assertions
/// against `pool`, then returns, after which the shared token cancels the server and consumer.
async fn drive<F, Fut>(pool: &PgPool, events: Vec<UserLifecycleEvent>, driver: F)
where
	F: FnOnce(PgPool) -> Fut,
	Fut: Future<Output = ()>, {
	// Bind once to claim a free ephemeral port, read it, then drop the listener and let tonic
	// re-bind the same addr — avoids a tokio-stream dep just to pass a pre-bound listener.
	// The bridge cursor is a single global row, so the two tests must not interleave their
	// drains. Serialize at the DB with a session advisory lock (held on a dedicated connection
	// for this drive) and reset the cursor so this drive pulls its own server from position 0.
	let mut guard = pool.acquire().await.expect("lock connection");
	sqlx::query("SELECT pg_advisory_lock($1)")
		.bind(BRIDGE_TEST_LOCK)
		.execute(guard.as_mut())
		.await
		.expect("take bridge test lock");
	sqlx::query("UPDATE bridge_cursor SET position = 0 WHERE id = TRUE").execute(pool).await.expect("reset cursor");

	let addr: SocketAddr = {
		let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind ephemeral port");
		listener.local_addr().unwrap()
	};
	let stop = CancellationToken::new();

	let server = {
		let stop = stop.clone();
		async move {
			Server::builder()
				.add_service(UserEventsServer::new(FakeUserEvents { events }))
				.serve_with_shutdown(addr, stop.cancelled_owned())
				.await
				.ok();
		}
	};

	let channel = tonic::transport::Endpoint::from_shared(format!("http://{addr}")).unwrap().connect_lazy();
	let consumer = BridgeConsumer::new(pool.clone(), channel, BRIDGE_TOKEN.to_string(), Duration::from_millis(50)).run(stop.clone());

	let asserter = {
		let pool = pool.clone();
		let stop = stop.clone();
		async move {
			// Let the server bind and the consumer poll/apply the backlog before asserting.
			tokio::time::sleep(Duration::from_millis(600)).await;
			driver(pool).await;
			stop.cancel();
		}
	};

	tokio::join!(server, consumer, asserter);

	// Release the session advisory lock (a pooled connection isn't closed, so it must be
	// unlocked explicitly) before the connection returns to the pool for the next drive.
	sqlx::query("SELECT pg_advisory_unlock($1)").bind(BRIDGE_TEST_LOCK).execute(guard.as_mut()).await.ok();
}

#[tokio::test]
async fn created_then_suspended_freezes_user_and_gates_money_op() {
	let Some(pool) = pool().await else {
		eprintln!("DATABASE_URL unset — skipping real-DB test");
		return;
	};
	let subject = unique_subject();
	let events = vec![event(&subject, Kind::Created, 1), event(&subject, Kind::Suspended, 2)];

	drive(&pool, events, |pool| async move {
		let user_id = user_id_for(&pool, &subject).await.expect("CREATED provisioned a banking user");
		assert!(
			bridge::is_frozen(&pool, user_id).await.unwrap(),
			"SUSPENDED must freeze the banking user — the money-op gate then rejects"
		);
		// And the issuance resolve reports the freeze, so AuthService.IssueUserToken refuses to
		// mint a money-plane token for a suspended user (defense in depth beyond the op gate).
		let target = PgUsers::new(pool.clone())
			.resolve_issuance_by_banking_id(domain::users::UserId::from_raw(user_id))
			.await
			.unwrap()
			.expect("resolve issuance");
		assert!(target.disabled, "a suspended user resolves as disabled → no money token is issued");
	})
	.await;
}

#[tokio::test]
async fn created_stores_concierge_id_and_resolves_for_issuance() {
	let Some(pool) = pool().await else {
		return;
	};
	let subject = unique_subject();
	let concierge_id = uuid::Uuid::new_v4();

	drive(&pool, vec![created_event(&subject, concierge_id)], move |pool| {
		let subject = subject.clone();
		async move {
			// The bridge stored the concierge user id on the mirror row — the issuance handle.
			let stored: Option<uuid::Uuid> = sqlx::query_scalar("SELECT concierge_user_id FROM users WHERE auth_subject = $1")
				.bind(&subject)
				.fetch_one(&pool)
				.await
				.unwrap();
			assert_eq!(stored, Some(concierge_id), "CREATED must store the concierge user id");

			// The issuance resolve path (AuthService.IssueUserToken → ResolveForIssuance) finds the
			// user by that concierge id, and the refresh path resolves the same row by hub id.
			let users = PgUsers::new(pool.clone());
			let by_concierge = users.resolve_issuance_by_concierge_id(concierge_id).await.unwrap().expect("resolved by concierge id");
			assert!(!by_concierge.disabled, "a freshly created user is not disabled");
			let by_banking = users.resolve_issuance_by_banking_id(by_concierge.user_id).await.unwrap().expect("resolved by hub id");
			assert_eq!(by_banking.user_id, by_concierge.user_id, "both lookups resolve the same hub user");
		}
	})
	.await;
}

#[tokio::test]
async fn redelivery_is_idempotent() {
	let Some(pool) = pool().await else {
		return;
	};
	let subject = unique_subject();
	// CREATED, SUSPENDED, then a stale REINSTATED with a LOWER sequence than the suspend —
	// the per-user order guard must drop it, so the user stays frozen.
	let events = vec![event(&subject, Kind::Created, 1), event(&subject, Kind::Suspended, 5), event(&subject, Kind::Reinstated, 3)];

	// First pass: apply the backlog. The stale lower-sequence reinstate must be dropped.
	let subject_a = subject.clone();
	drive(&pool, events.clone(), |pool| async move {
		let user_id = user_id_for(&pool, &subject_a).await.expect("provisioned");
		let seq: i64 = sqlx::query_scalar("SELECT last_lifecycle_sequence FROM users WHERE id = $1")
			.bind(user_id)
			.fetch_one(&pool)
			.await
			.unwrap();
		assert_eq!(seq, 5, "applied through the suspend; the stale lower-sequence reinstate is dropped");
		assert!(bridge::is_frozen(&pool, user_id).await.unwrap(), "stale REINSTATED must not un-freeze");
	})
	.await;

	// Second pass: `drive` resets the cursor to 0, so a fresh consumer re-pulls and re-applies
	// the SAME events — dedupe by per-user sequence must make every re-apply a no-op (frozen
	// stays frozen, sequence stays 5).
	drive(&pool, events, |pool| async move {
		let user_id = user_id_for(&pool, &subject).await.expect("provisioned");
		let seq: i64 = sqlx::query_scalar("SELECT last_lifecycle_sequence FROM users WHERE id = $1")
			.bind(user_id)
			.fetch_one(&pool)
			.await
			.unwrap();
		assert_eq!(seq, 5, "redelivery is a no-op — sequence does not move");
		assert!(bridge::is_frozen(&pool, user_id).await.unwrap(), "redelivery keeps the user frozen");
	})
	.await;
}
