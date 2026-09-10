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
		// Base events carry no role (empty → the consumer mirrors it as Investor); a
		// ROLE_CHANGED test overrides this field explicitly.
		role: String::new(),
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
			bridge::is_frozen(&pool, domain::users::UserId::from_raw(user_id)).await.unwrap(),
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
async fn banking_disable_blocks_money_ops_without_a_concierge_freeze() {
	let Some(pool) = pool().await else {
		return;
	};
	let subject = unique_subject();

	drive(&pool, vec![event(&subject, Kind::Created, 1)], |pool| async move {
		let user_id = user_id_for(&pool, &subject).await.expect("CREATED provisioned a banking user");
		// Not concierge-frozen, so nothing blocks money ops yet.
		assert!(!bridge::is_frozen(&pool, domain::users::UserId::from_raw(user_id)).await.unwrap(), "a fresh user is not blocked");

		// A banking-side DisableUser sets status='disabled' and leaves `frozen` FALSE. The
		// money-op gate must still block it, matching the fold issuance already applies.
		sqlx::query("UPDATE users SET status = 'disabled' WHERE id = $1").bind(user_id).execute(&pool).await.unwrap();
		assert!(
			bridge::is_frozen(&pool, domain::users::UserId::from_raw(user_id)).await.unwrap(),
			"a banking-disabled user must be blocked from money ops, like a concierge freeze"
		);
	})
	.await;
}

#[tokio::test]
async fn role_changed_mirrors_role_onto_the_projection() {
	let Some(pool) = pool().await else {
		return;
	};
	let subject = unique_subject();
	// CREATED (empty role → Investor) then a ROLE_CHANGED promoting to admin. The money
	// plane's operator gate reads exactly this mirrored column.
	let mut promote = event(&subject, Kind::RoleChanged, 2);
	promote.role = "admin".to_string();
	let events = vec![event(&subject, Kind::Created, 1), promote];

	drive(&pool, events, move |pool| {
		let subject = subject.clone();
		async move {
			let role: String = sqlx::query_scalar("SELECT role FROM users WHERE auth_subject = $1")
				.bind(&subject)
				.fetch_one(&pool)
				.await
				.unwrap();
			assert_eq!(role, "admin", "ROLE_CHANGED mirrors the granted role onto the banking projection");
			let user_id = user_id_for(&pool, &subject).await.expect("user provisioned");
			assert_eq!(
				bridge::role_of(&pool, domain::users::UserId::from_raw(user_id)).await.unwrap(),
				domain::authz::Role::Admin,
				"role_of reads it back for the operator gate"
			);
		}
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
			let by_concierge = users
				.resolve_issuance_by_concierge_id(domain::users::ConciergeUserId::from_raw(concierge_id))
				.await
				.unwrap()
				.expect("resolved by concierge id");
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
		assert!(
			bridge::is_frozen(&pool, domain::users::UserId::from_raw(user_id)).await.unwrap(),
			"stale REINSTATED must not un-freeze"
		);
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
		assert!(
			bridge::is_frozen(&pool, domain::users::UserId::from_raw(user_id)).await.unwrap(),
			"redelivery keeps the user frozen"
		);
	})
	.await;
}

/// THE MIRRORED COLUMN IS THE ONLY SOURCE OF THE ROLE.
///
/// The money plane used to promote subjects listed in an `ADMIN_SUBJECTS` env var to
/// `Role::Owner` inside `require_permission`, ahead of this lookup. That produced owners
/// the consilium could not count (it reads the persisted roster) and a second UUID list an
/// operator had to keep in sync with concierge's by hand. The override is gone: a subject
/// with no row in the local projection resolves to `Role::default()` and holds nothing —
/// least of all the permission that pays the fund's own revenue out.
#[tokio::test]
async fn a_subject_absent_from_the_projection_is_never_an_owner() {
	let Some(pool) = pool().await else {
		return;
	};
	// Never provisioned by the bridge, so no `users` row exists for it.
	let stranger = domain::users::UserId::from_raw(uuid::Uuid::new_v4());

	let role = bridge::role_of(&pool, stranger).await.unwrap();
	assert_eq!(role, domain::authz::Role::default(), "no local row ⇒ the default role, not an inherited privilege");
	assert_ne!(role, domain::authz::Role::Owner, "nothing outside the mirrored column may grant ownership");
	assert!(
		!domain::authz::grants(role, domain::authz::Permission::RevenuePayout),
		"an unknown subject must not be able to move the fund's revenue"
	);
}

/// AN OWNER MUST NEVER REACH THE MONEY PLANE WITHOUT A ROSTER-JOURNAL ROW.
///
/// This event shape does not occur today: concierge stamps the role snapshot at drain time,
/// and CREATED drains right after `User::provision`, while the role is still `investor`. The
/// arm is a latch against that ordering changing in the other repository. Without the
/// journal row there is no `changed_at` for the cooling-off window to read, so a roster
/// change would be immediately launderable into a payout — the exact motion the 48h freeze
/// exists to make visible. If this test ever looks redundant, that is the argument for
/// keeping it, not for deleting it.
#[tokio::test]
async fn created_carrying_owner_still_journals_the_roster_change() {
	let Some(pool) = pool().await else {
		return;
	};
	let subject = unique_subject();
	let mut born_an_owner = event(&subject, Kind::Created, 1);
	born_an_owner.role = "owner".to_string();

	drive(&pool, vec![born_an_owner], move |pool| {
		let subject = subject.clone();
		async move {
			let user_id = user_id_for(&pool, &subject).await.expect("user provisioned");
			let journalled: Option<(String, String)> = sqlx::query_as("SELECT from_role, to_role FROM governance_roster_change WHERE user_id = $1")
				.bind(user_id)
				.fetch_optional(&pool)
				.await
				.unwrap();
			assert_eq!(
				journalled,
				Some(("investor".to_string(), "owner".to_string())),
				"a CREATED that seats an owner must start the cooling-off clock, reporting the nothing it replaced as the default role"
			);
		}
	})
	.await;
}

/// The common case must NOT be journalled: almost every CREATED carries `investor`, and
/// charging the cooling-off window for each new signup would freeze payouts permanently.
#[tokio::test]
async fn created_carrying_an_ordinary_role_journals_nothing() {
	let Some(pool) = pool().await else {
		return;
	};
	let subject = unique_subject();

	drive(&pool, vec![event(&subject, Kind::Created, 1)], move |pool| {
		let subject = subject.clone();
		async move {
			let user_id = user_id_for(&pool, &subject).await.expect("user provisioned");
			let journalled: i64 = sqlx::query_scalar("SELECT count(*) FROM governance_roster_change WHERE user_id = $1")
				.bind(user_id)
				.fetch_one(&pool)
				.await
				.unwrap();
			assert_eq!(journalled, 0, "an ordinary signup moves nobody in or out of the voting roster");
		}
	})
	.await;
}

/// Poll `check` until it holds or the budget runs out, then return whether it held. The
/// consumer runs concurrently on its own poll interval, so an assertion about work it has
/// yet to do needs to wait for a cycle rather than race one.
async fn eventually<F, Fut>(mut check: F) -> bool
where
	F: FnMut() -> Fut,
	Fut: Future<Output = bool>, {
	for _ in 0..40 {
		if check().await {
			return true;
		}
		tokio::time::sleep(Duration::from_millis(50)).await;
	}
	false
}

async fn parked_count(pool: &PgPool, subject: &str) -> i64 {
	sqlx::query_scalar("SELECT count(*) FROM bridge_deferred_event WHERE auth_subject = $1")
		.bind(subject)
		.fetch_one(pool)
		.await
		.unwrap()
}

async fn cursor_position(pool: &PgPool) -> i64 {
	sqlx::query_scalar("SELECT position FROM bridge_cursor WHERE id = TRUE").fetch_one(pool).await.unwrap()
}

async fn sequence_of(pool: &PgPool, subject: &str) -> i64 {
	sqlx::query_scalar("SELECT last_lifecycle_sequence FROM users WHERE auth_subject = $1")
		.bind(subject)
		.fetch_one(pool)
		.await
		.unwrap()
}

/// THE TIER MIRROR, END TO END — the path that had no test at all.
///
/// `users.kyc_level` is what every money gate reads (`application::wallet::is_verified`,
/// the withdrawal admission, the payout standing). Nothing else writes it: the concierge
/// plane owns the tier and this event is the only way it crosses. A regression here does
/// not fail loudly — it leaves users at the schema default of 0, silently locked out of
/// deposit addresses and withdrawals.
#[tokio::test]
async fn kyc_changed_mirrors_the_tier_the_money_gates_read() {
	let Some(pool) = pool().await else {
		return;
	};
	let subject = unique_subject();
	// A bare registration (level 0 — a confirmed email and nothing else), verified afterwards.
	let mut registered = event(&subject, Kind::Created, 1);
	registered.kyc_level = 0;
	let mut verified = event(&subject, Kind::KycChanged, 2);
	verified.kyc_level = domain::users::KYC_LEVEL_VERIFIED;
	let events = vec![registered, verified];

	drive(&pool, events, move |pool| {
		let subject = subject.clone();
		async move {
			let user_id = user_id_for(&pool, &subject).await.expect("CREATED provisioned a banking user");
			let level: i32 = sqlx::query_scalar("SELECT kyc_level FROM users WHERE id = $1").bind(user_id).fetch_one(&pool).await.unwrap();
			assert_eq!(level as u32, domain::users::KYC_LEVEL_VERIFIED, "KYC_CHANGED mirrors the tier onto the banking projection");

			// And the gates read it back through the repository they actually use, not the
			// column directly — an aggregate that dropped the field on the way out would still
			// leave the user locked out.
			let account = PgUsers::new(pool.clone())
				.find_by_id(domain::users::UserId::from_raw(user_id))
				.await
				.unwrap()
				.expect("the mirrored user loads");
			assert!(
				account.kyc_level() >= domain::users::KYC_LEVEL_VERIFIED,
				"the mirrored tier must clear the verification gate the wallet and withdrawal paths apply"
			);
		}
	})
	.await;
}

/// AN EVENT FOR A SUBJECT BANKING HAS NEVER SEEN MUST SURVIVE UNTIL IT CAN BE APPLIED.
///
/// The cursor is global and the concierge only ever re-delivers ahead of it, so an event
/// the consumer walks past is gone for good. `apply` used to walk past every event whose
/// subject had no local row — a user who has not signed in here yet, or anyone at all if
/// banking was deployed after the concierge and their CREATED has already aged out of the
/// outbox. The tier below would have vanished and left the user at level 0, locked out of
/// both a deposit address and a withdrawal, until the concierge happened to move their
/// tier again.
#[tokio::test]
async fn an_event_for_an_unknown_subject_is_parked_and_replayed_when_the_row_appears() {
	let Some(pool) = pool().await else {
		return;
	};
	let subject = unique_subject();
	// No CREATED — only the tier change reaches this deployment.
	let mut verified = event(&subject, Kind::KycChanged, 2);
	verified.kyc_level = domain::users::KYC_LEVEL_VERIFIED;

	drive(&pool, vec![verified], move |pool| {
		let subject = subject.clone();
		async move {
			assert!(user_id_for(&pool, &subject).await.is_none(), "nothing has provisioned this subject");
			assert_eq!(parked_count(&pool, &subject).await, 1, "an event for an unknown subject is parked, not consumed into the void");
			// The cursor is deliberately NOT held here: the event is durable on this side, so
			// one orphan subject must not wedge the mirror for every other user.
			assert_eq!(cursor_position(&pool).await, 1, "parking keeps the stream moving");

			// First sign-in materializes the row — at the schema default of level 0, which is
			// exactly the lock-out the dropped event used to leave behind.
			PgUsers::new(pool.clone())
				.provision(
					domain::auth::AuthSubject::parse(&subject).unwrap(),
					domain::users::Email::parse("bridged@example.com").unwrap(),
					true,
				)
				.await
				.expect("first sign-in provisions the row");

			let mirrored = eventually(|| {
				let (pool, subject) = (pool.clone(), subject.clone());
				async move {
					let level: i32 = sqlx::query_scalar("SELECT kyc_level FROM users WHERE auth_subject = $1")
						.bind(&subject)
						.fetch_one(&pool)
						.await
						.unwrap();
					level as u32 == domain::users::KYC_LEVEL_VERIFIED
				}
			})
			.await;
			assert!(mirrored, "the parked tier must be replayed onto the row the moment it exists");
			assert_eq!(parked_count(&pool, &subject).await, 0, "a replayed event is released from the parking lot");
		}
	})
	.await;
}

/// A KIND THIS BUILD CANNOT NAME MUST NOT BE RECORDED AS APPLIED.
///
/// The concierge ships ahead of banking, so a kind newer than the pinned contracts is the
/// ordinary shape of a mid-rollout event, not a corruption. Bumping `last_lifecycle_sequence`
/// for it — the old "so it isn't re-fetched forever" behaviour — put the event behind the
/// per-user guard, where the upgraded build could never reach it: if the new kind meant a
/// freeze or a tier revocation, the money plane would go on trading under rules the identity
/// plane had already withdrawn. Nothing local can interpret it, so the cursor stops instead.
#[tokio::test]
async fn an_unreadable_kind_holds_the_cursor_until_a_build_that_understands_it() {
	let Some(pool) = pool().await else {
		return;
	};
	let subject = unique_subject();
	// A kind number outside this build's enum, exactly as an older binary would decode one.
	let mut from_the_future = event(&subject, Kind::Suspended, 2);
	from_the_future.kind = 99;
	// A third event BEHIND the unreadable one: it must not be applied either, or it would
	// advance the per-user guard past the event the stop exists to preserve.
	let mut later = event(&subject, Kind::KycChanged, 3);
	later.kyc_level = domain::users::KYC_LEVEL_VERIFIED;
	// Registered but not yet verified, so the tier the stop withholds is visibly absent.
	let mut registered = event(&subject, Kind::Created, 1);
	registered.kyc_level = 0;
	let events = vec![registered, from_the_future, later];

	let before = subject.clone();
	drive(&pool, events.clone(), move |pool| {
		let subject = before;
		async move {
			assert_eq!(sequence_of(&pool, &subject).await, 1, "the unreadable event must not be marked applied");
			assert_eq!(cursor_position(&pool).await, 0, "and the batch carrying it must not be consumed");
			assert_eq!(parked_count(&pool, &subject).await, 0, "an unreadable event is not parked — only a newer binary can read it");
			let level: i32 = sqlx::query_scalar("SELECT kyc_level FROM users WHERE auth_subject = $1")
				.bind(&subject)
				.fetch_one(&pool)
				.await
				.unwrap();
			assert_ne!(level as u32, domain::users::KYC_LEVEL_VERIFIED, "nothing behind the stop is applied either");
		}
	})
	.await;

	// The upgrade: the same outbox rows, at the same positions, decoded by a build that now
	// names the kind. Because nothing above was consumed, they are all still on offer.
	let mut understood = events;
	understood[1].kind = Kind::Suspended as i32;
	drive(&pool, understood, move |pool| {
		let subject = subject.clone();
		async move {
			let user_id = user_id_for(&pool, &subject).await.expect("provisioned by the first pass");
			assert!(
				bridge::is_frozen(&pool, domain::users::UserId::from_raw(user_id)).await.unwrap(),
				"the freeze that was unreadable before must land after the upgrade"
			);
			assert_eq!(sequence_of(&pool, &subject).await, 3, "and the events behind it apply in order");
			assert_eq!(cursor_position(&pool).await, 3, "the whole batch is consumed once every event in it is readable");
		}
	})
	.await;
}

async fn email_of(pool: &PgPool, subject: &str) -> (String, bool) {
	sqlx::query_as("SELECT email, email_verified FROM users WHERE auth_subject = $1")
		.bind(subject)
		.fetch_one(pool)
		.await
		.unwrap()
}

/// AN EMAIL CHANGE HAS NO EVENT OF ITS OWN — THE SNAPSHOT IS THE WHOLE DELIVERY MECHANISM.
///
/// concierge's `change_email` bumps `row_version` without `bump_and_emit`, on the stated
/// contract that banking re-reads the address from the snapshot on the next lifecycle event.
/// Banking never did: `email` was written once, by the CREATED insert, under
/// `ON CONFLICT DO NOTHING`, and no other arm touched it — so a user who changed their
/// address at the IdP kept their old one on the money plane forever, on statements, notices
/// and every operator screen (EV-invest/concierge#46).
#[tokio::test]
async fn a_changed_address_arrives_on_the_next_lifecycle_event() {
	let Some(pool) = pool().await else {
		eprintln!("DATABASE_URL unset — skipping real-DB test");
		return;
	};
	let subject = unique_subject();
	let mut registered = event(&subject, Kind::Created, 1);
	registered.email = "before@example.com".into();
	// The address changed at the IdP between the two; only the KYC move is emitted, and it
	// carries the new address as part of its snapshot.
	let mut verified = event(&subject, Kind::KycChanged, 2);
	verified.email = "after@example.com".into();
	verified.kyc_level = domain::users::KYC_LEVEL_VERIFIED;

	drive(&pool, vec![registered, verified], move |pool| {
		let subject = subject.clone();
		async move {
			let (email, _) = email_of(&pool, &subject).await;
			assert_eq!(email, "after@example.com", "the snapshot on a non-CREATED event must refresh the mirrored address");

			// And the aggregate still loads: a mirrored address that no longer parses would
			// take the user's whole profile and money surface down with it.
			let user_id = user_id_for(&pool, &subject).await.expect("provisioned");
			let account = PgUsers::new(pool.clone())
				.find_by_id(domain::users::UserId::from_raw(user_id))
				.await
				.unwrap()
				.expect("the mirrored user loads");
			assert_eq!(account.email().as_str(), "after@example.com", "the repository reads back the refreshed address");
		}
	})
	.await;
}

/// The verification flag rides the same snapshot, and on a kind that has nothing to do with
/// either — the refresh is kind-independent by construction, not a KYC_CHANGED special case.
#[tokio::test]
async fn the_verification_flag_travels_with_the_address() {
	let Some(pool) = pool().await else {
		return;
	};
	let subject = unique_subject();
	let mut registered = event(&subject, Kind::Created, 1);
	registered.email = "unconfirmed@example.com".into();
	registered.email_verified = false;
	// A revoke says nothing about the address — but it carries the snapshot, so it delivers it.
	let mut revoked = event(&subject, Kind::SessionsRevoked, 2);
	revoked.email = "confirmed@example.com".into();
	revoked.email_verified = true;

	drive(&pool, vec![registered, revoked], move |pool| {
		let subject = subject.clone();
		async move {
			assert_eq!(
				email_of(&pool, &subject).await,
				("confirmed@example.com".to_string(), true),
				"any lifecycle kind carrying the snapshot refreshes both the address and its verification flag"
			);
		}
	})
	.await;
}

/// A PARKED EVENT CARRIES A SNAPSHOT FROM BEFORE THE ROW EXISTED, AND MUST NOT WRITE IT BACK.
///
/// Parking (migration 0028) means the subject had NO local row when the event arrived. The
/// only thing that creates one afterwards is a first sign-in, which writes the IdP's live
/// address — so by the time the replay runs, the row is strictly newer than the snapshot the
/// replay is holding. Mirroring it anyway would roll the address back to whatever it was at
/// the moment concierge drained the event, which for a bootstrap backlog can be months.
#[tokio::test]
async fn a_replayed_parked_event_does_not_roll_the_address_back() {
	let Some(pool) = pool().await else {
		return;
	};
	let subject = unique_subject();
	// No CREATED reaches this deployment — only a tier change, snapshotting the OLD address.
	let mut verified = event(&subject, Kind::KycChanged, 2);
	verified.email = "stale@example.com".into();
	verified.kyc_level = domain::users::KYC_LEVEL_VERIFIED;

	drive(&pool, vec![verified], move |pool| {
		let subject = subject.clone();
		async move {
			assert_eq!(parked_count(&pool, &subject).await, 1, "an event for an unknown subject is parked");

			// First sign-in materializes the row with the address the IdP holds NOW.
			PgUsers::new(pool.clone())
				.provision(
					domain::auth::AuthSubject::parse(&subject).unwrap(),
					domain::users::Email::parse("current@example.com").unwrap(),
					true,
				)
				.await
				.expect("first sign-in provisions the row");

			let replayed = eventually(|| {
				let (pool, subject) = (pool.clone(), subject.clone());
				async move { parked_count(&pool, &subject).await == 0 }
			})
			.await;
			assert!(replayed, "the parked event must be released once the row exists");

			let (email, _) = email_of(&pool, &subject).await;
			assert_eq!(email, "current@example.com", "a replayed snapshot must not overwrite the newer address the row already holds");
			let level: i32 = sqlx::query_scalar("SELECT kyc_level FROM users WHERE auth_subject = $1")
				.bind(&subject)
				.fetch_one(&pool)
				.await
				.unwrap();
			assert_eq!(
				level as u32,
				domain::users::KYC_LEVEL_VERIFIED,
				"the state-machine fields it carries still apply — only the snapshot is withheld"
			);
		}
	})
	.await;
}
