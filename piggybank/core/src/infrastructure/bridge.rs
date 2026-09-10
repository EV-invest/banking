//! Cross-plane lifecycle bridge — the consumer side of the ONE-WAY identity→money seam.
//!
//! A single background task periodically PULLS `UserLifecycleEvent`s from the concierge
//! plane (`UserEvents.PullUserLifecycle`, authenticated with the shared
//! `BRIDGE_SERVICE_TOKEN`) and applies each to the banking `users` control plane, so a
//! concierge SUSPENDED/REINSTATED/KYC/revoke is mirrored here and money ops can be gated.
//! Concierge never calls banking — banking pulls.
//!
//! Delivery is at-least-once, so the consumer is idempotent:
//!   - dedupe + ORDER by the per-user `sequence` — an event applies only when its
//!     `sequence` exceeds `users.last_lifecycle_sequence`, so a redelivery is a no-op and a
//!     stale REINSTATED can't un-freeze a user a later SUSPENDED already froze;
//!   - the global `bridge_cursor.position` advances to the batch's `next_position` ONLY
//!     after every event in the batch is applied, so a crash mid-batch re-pulls and the
//!     per-user guard absorbs the re-apply.
//!
//! Not everything that crosses is a state machine. `frozen`, `kyc_level`, `role` and the
//! revoke floor each have a kind of their own and are ordered by `sequence`; the EMAIL has
//! neither. Concierge's `change_email` deliberately emits no event, on the contract that the
//! profile snapshot riding on the NEXT lifecycle row carries the change — so the snapshot is
//! applied here on every pulled event, whatever its kind, and never in a per-kind arm that a
//! new kind could forget to copy. A REPLAYED event is the exception: see [`Freshness`].
//!
//! Correlation is by `auth_subject` (the provider `sub` both planes provision against),
//! never concierge's own `user_id` — a CREATED event provisions a minimal local row for an
//! as-yet-unseen subject (banking otherwise materializes a user on first sign-in).
//!
//! NOTHING IS EVER CONSUMED WITHOUT BEING APPLIED. The cursor is a single global position
//! and the concierge never re-delivers behind it (`WHERE position > after_position`), so
//! every event that cannot be applied right now needs somewhere to go. There are exactly
//! two reasons an event cannot be applied, and they get opposite treatments because they
//! are unblocked by opposite things:
//!   - **The subject has no local row yet** — unblocked by that row appearing, which may
//!     be minutes away (first sign-in) or never (a CREATED that fell out of the outbox
//!     window before banking was deployed). Parked in `bridge_deferred_event` and replayed
//!     by [`BridgeConsumer::replay_deferred`] once the row exists; the cursor moves on, so
//!     one orphan subject cannot wedge the mirror for everyone else.
//!   - **The kind is one this build cannot name** — concierge ships ahead of banking, so a
//!     `Kind::Unspecified` is the ordinary shape of a mid-rollout event, and it is
//!     unblocked only by upgrading this binary. Nothing local can replay it, so the cursor
//!     STOPS on it (head-of-line) and the event stays in concierge's outbox at full
//!     fidelity until a build that understands it pulls it. Marking it applied instead is
//!     how a tier downgrade or a freeze could be swallowed while the money plane keeps
//!     running under rules the identity plane already revoked.

use std::time::Duration;

use domain::{
	authz::Role,
	users::{Email, UserId},
};
use evconcierge_contracts::concierge::v1::{PullUserLifecycleRequest, UserLifecycleEvent, user_events_client::UserEventsClient, user_lifecycle_event::Kind};
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use tonic::{Request, metadata::MetadataValue, transport::Channel};
use tracing::{info, warn};

/// How many outbox rows to request per pull. The server caps `limit` at its own ceiling
/// (500), so this is the steady-state batch, not a hard bound.
const PULL_LIMIT: u32 = 256;

/// What [`BridgeConsumer::apply`] did with one event. `drain` reads this to decide whether
/// the single global cursor may move past it — the cursor is the only delivery guarantee
/// there is, so this is a consumed/not-consumed verdict, not a status log.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Outcome {
	/// Mirrored onto the local row, or dropped as a redelivery the per-user sequence guard
	/// already covers. Either way this build is done with it: the cursor may pass.
	Mirrored,
	/// Parked in `bridge_deferred_event` because the subject has no local row yet (or has an
	/// earlier event still parked). Durable on this side and owned by `replay_deferred`, so
	/// the cursor may pass.
	Parked,
	/// A kind this build's pinned contracts cannot name. The cursor must NOT pass: only a
	/// re-pull by an upgraded binary can ever apply it.
	Unreadable,
}

/// Whether an event's PROFILE SNAPSHOT (email, verification flag) still describes the
/// subject, as opposed to the state-machine fields (`frozen`, `kyc_level`, `role`,
/// `concierge_token_version`), which are ordered by `sequence` and safe either way.
///
/// The snapshot is taken when concierge DRAINS the event, not when banking applies it, so
/// the two are only interchangeable while the gap is small.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Freshness {
	/// Pulled from the concierge outbox moments ago: the newest snapshot either plane holds.
	Live,
	/// Replayed out of `bridge_deferred_event`. It was parked because the subject had NO
	/// local row, and the only thing that can create one afterwards is a first sign-in
	/// (`PgUsers::provision`), which writes the IdP's live address. So by construction the
	/// row this replay lands on already carries an address newer than the snapshot, and the
	/// snapshot must not be written back over it.
	Stale,
}

/// The bridge consumer task: pull → apply → advance the cursor, on a poll interval.
pub struct BridgeConsumer {
	pool: PgPool,
	channel: Channel,
	service_token: String,
	poll_interval: Duration,
}

impl BridgeConsumer {
	pub fn new(pool: PgPool, channel: Channel, service_token: String, poll_interval: Duration) -> Self {
		Self {
			pool,
			channel,
			service_token,
			poll_interval,
		}
	}

	/// Run until `shutdown` is cancelled. Each cycle drains the available backlog, then
	/// waits the poll interval (or wakes on cancellation). A transient pull/apply failure is
	/// logged and retried next cycle from the unchanged cursor — nothing is dropped.
	pub async fn run(self, shutdown: CancellationToken) {
		info!(every = ?self.poll_interval, "bridge: consuming concierge lifecycle events");
		let mut client = UserEventsClient::new(self.channel.clone());
		loop {
			if let Err(err) = self.drain(&mut client).await {
				let hint = match err.downcast_ref::<tonic::Status>() {
					Some(s) if s.code() == tonic::Code::Unavailable => " (is concierge running?)",
					_ => "",
				};
				warn!("bridge: pull/apply cycle failed, retrying next poll{hint}: {err}");
			}
			tokio::select! {
				() = shutdown.cancelled() => {
					info!("bridge: shutdown requested — stopping");
					return;
				},
				() = tokio::time::sleep(self.poll_interval) => {},
			}
		}
	}

	/// Drain the available backlog: repeatedly pull from the stored cursor and apply each
	/// batch until the server returns no new rows (`next_position` unchanged).
	///
	/// The parked backlog is swept after every pass, and once even when there was nothing to
	/// pull: a row can appear from the *other* direction (first sign-in materializes a user
	/// this bridge never provisioned), and that must not wait for the next concierge event.
	async fn drain(&self, client: &mut UserEventsClient<Channel>) -> color_eyre::Result<()> {
		loop {
			let more = self.pull_and_apply(client).await?;
			self.replay_deferred().await?;
			if !more {
				return Ok(());
			}
		}
	}

	/// Pull one batch from the stored cursor and apply it. Returns whether a full batch was
	/// consumed, i.e. whether more may be waiting behind it.
	async fn pull_and_apply(&self, client: &mut UserEventsClient<Channel>) -> color_eyre::Result<bool> {
		let after = self.cursor().await?;
		let mut request = Request::new(PullUserLifecycleRequest {
			after_position: after,
			limit: PULL_LIMIT,
		});
		let token: MetadataValue<_> = format!("Bearer {}", self.service_token).parse()?;
		request.metadata_mut().insert("authorization", token);

		let response = client.pull_user_lifecycle(request).await?.into_inner();
		if response.events.is_empty() {
			return Ok(false);
		}
		for event in &response.events {
			if self.apply(event, Freshness::Live).await? == Outcome::Unreadable {
				// HEAD-OF-LINE STOP. RETURNING HERE IS LOAD-BEARING TWICE OVER.
				//
				// Leaving the cursor put is what keeps the event in concierge's outbox for an
				// upgraded build to pull. Abandoning the REST of the batch is what keeps a
				// later event for the same subject from advancing that subject's
				// `last_lifecycle_sequence` past this one — which would hide it behind the
				// per-user guard forever, reinstating exactly the loss the stop prevents.
				// Everything already applied from this batch re-applies as a no-op when the
				// unchanged cursor re-delivers it.
				return Ok(false);
			}
		}
		self.advance_cursor(after, response.next_position).await?;
		// A short batch (server gave back fewer than it caps) means we caught up.
		Ok((response.events.len() as u32) >= PULL_LIMIT)
	}

	/// Replay parked events for every subject that now has a local row, oldest first.
	///
	/// Ordering by `(auth_subject, sequence)` matters: the per-user guard only ever moves
	/// forward, so replaying a subject's backlog out of order would drop its earlier events.
	/// A row is deleted only once its event actually mirrored — a parked event is the sole
	/// remaining copy, since the concierge cursor has long since moved past it.
	async fn replay_deferred(&self) -> Result<(), sqlx::Error> {
		let parked: Vec<DeferredEvent> = sqlx::query_as(
			"SELECT d.event_id, d.auth_subject, d.kind, d.sequence, d.concierge_user_id, d.email, d.email_verified, d.kyc_level, d.role, d.token_version, d.occurred_at \
			 FROM bridge_deferred_event d JOIN users u ON u.auth_subject = d.auth_subject ORDER BY d.auth_subject, d.sequence",
		)
		.fetch_all(&self.pool)
		.await?;
		if parked.is_empty() {
			return Ok(());
		}
		info!(count = parked.len(), "bridge: replaying deferred lifecycle events whose subject now exists");
		for row in parked {
			let event_id = row.event_id.clone();
			if self.apply(&row.into_event(), Freshness::Stale).await? != Outcome::Mirrored {
				// Re-parked: the row was deleted again between the join and the apply, or an
				// earlier sibling is still waiting. Leave this one where it is.
				continue;
			}
			sqlx::query("DELETE FROM bridge_deferred_event WHERE event_id = $1").bind(&event_id).execute(&self.pool).await?;
		}
		Ok(())
	}

	async fn cursor(&self) -> Result<i64, sqlx::Error> {
		sqlx::query_scalar::<_, i64>("SELECT position FROM bridge_cursor WHERE id = TRUE").fetch_one(&self.pool).await
	}

	/// Advance the cursor only after the batch applied. The `WHERE position = $1` guard makes
	/// the write a no-op if a concurrent consumer already moved it (there is one consumer, but
	/// this keeps the advance monotonic and crash-safe) and never moves it backwards.
	async fn advance_cursor(&self, from: i64, to: i64) -> Result<(), sqlx::Error> {
		if to <= from {
			return Ok(());
		}
		sqlx::query("UPDATE bridge_cursor SET position = $2, updated_at = now() WHERE id = TRUE AND position = $1")
			.bind(from)
			.bind(to)
			.execute(&self.pool)
			.await?;
		Ok(())
	}

	/// Apply one event idempotently, in a transaction: take the per-user row lock, skip if its
	/// `sequence` doesn't advance `last_lifecycle_sequence`, else mutate by `kind` and stamp the
	/// new sequence. CREATED provisions a minimal row for an unseen subject. An event for a
	/// subject with no local row is parked for replay; an unnameable `kind` is refused outright
	/// so the caller can stop the cursor. See the module header for why those differ.
	///
	/// A `Freshness::Live` event also refreshes the profile snapshot (email) regardless of its
	/// kind — see [`refresh_profile_snapshot`].
	async fn apply(&self, event: &UserLifecycleEvent, freshness: Freshness) -> Result<Outcome, sqlx::Error> {
		let subject = &event.auth_subject;
		let sequence = event.sequence as i64;

		if event.kind() == Kind::Unspecified {
			// A kind newer than this build's pinned contracts. It used to bump
			// `last_lifecycle_sequence` here "so it isn't re-fetched forever" — but that guard
			// doubles as the journal of what has been applied, so skipping and applying became
			// indistinguishable and the event was swallowed permanently. Nothing is written,
			// and the caller holds the cursor; the re-fetching IS the alert, and it ends when
			// the deploy that understands the kind lands.
			warn!(
				event_id = %event.event_id,
				kind = event.kind,
				subject = %subject,
				sequence,
				"bridge: unreadable lifecycle kind — holding the cursor for a build that knows it"
			);
			return Ok(Outcome::Unreadable);
		}

		// Concierge's own user id — the handle the BFF later presents on IssueUserToken. A
		// malformed value (should never happen) is stored as NULL rather than failing the event.
		let concierge_user_id = uuid::Uuid::parse_str(&event.user_id).ok();
		let mut tx = self.pool.begin().await?;

		// A subject with an EARLIER event still parked keeps parking, ahead of any mutation —
		// including CREATED's insert. Otherwise a row materialized mid-stream by first sign-in
		// would let a later event apply, advance the per-user guard, and bury the parked
		// earlier one behind it. Strict `<` excludes the event being replayed from its own
		// check, which is what lets `replay_deferred` drain a backlog at all.
		let blocked: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM bridge_deferred_event WHERE auth_subject = $1 AND sequence < $2)")
			.bind(subject)
			.bind(sequence)
			.fetch_one(&mut *tx)
			.await?;
		if blocked {
			defer(&mut tx, event, concierge_user_id).await?;
			tx.commit().await?;
			return Ok(Outcome::Parked);
		}

		// The role snapshot rides on every lifecycle row; an older concierge (or a
		// pre-role row) carries an empty value that degrades to Investor.
		let role = Role::parse_or_default(&event.role);
		if event.kind() == Kind::Created {
			// `RETURNING id` yields a row ONLY when this statement actually inserted;
			// `ON CONFLICT DO NOTHING` returns nothing when the row already existed.
			let seated: Option<uuid::Uuid> = sqlx::query_scalar(
				"INSERT INTO users (id, auth_subject, concierge_user_id, email, email_verified, kyc_level, role, last_lifecycle_sequence) \
				 VALUES (gen_random_uuid(), $1, $2, $3, $4, $5, $6, $7) ON CONFLICT (auth_subject) DO NOTHING RETURNING id",
			)
			.bind(subject)
			.bind(concierge_user_id)
			.bind(&event.email)
			.bind(event.email_verified)
			.bind(event.kyc_level as i32)
			.bind(role.as_str())
			.bind(sequence)
			.fetch_optional(&mut *tx)
			.await?;

			// THE JOURNAL WRITE BELONGS HERE, NOT ONLY IN THE `Kind::Created` MATCH ARM.
			//
			// This INSERT stamps `last_lifecycle_sequence` itself, so the sequence guard below
			// sees `sequence == current` and returns early — the match arm never runs for a row
			// this event just created. Putting the journal write only there would have left the
			// latch permanently dead for exactly the case it exists to catch.
			//
			// A row that did not exist held nothing, so the role it replaced is the default,
			// which is also what keeps the table's `from_role <> to_role` CHECK satisfied.
			if let Some(user_id) = seated {
				record_roster_change(&mut tx, user_id, Role::default().as_str(), role.as_str()).await?;
			}
		}

		let current: Option<i64> = sqlx::query_scalar("SELECT last_lifecycle_sequence FROM users WHERE auth_subject = $1 FOR UPDATE")
			.bind(subject)
			.fetch_optional(&mut *tx)
			.await?;
		let Some(current) = current else {
			// No local row and not a CREATED (or CREATED lost the insert race and the row is
			// being built by another path) — nothing to mutate YET. This used to return here
			// and let `drain` advance the cursor past the event, which consumed it into the
			// void: the concierge only re-delivers ahead of the cursor, so a KYC_CHANGED left
			// the user at level 0 and locked out, and a SUSPENDED left the row created at
			// sign-in with `frozen = FALSE` — fail-open on a freeze. Park it instead.
			defer(&mut tx, event, concierge_user_id).await?;
			tx.commit().await?;
			return Ok(Outcome::Parked);
		};
		if sequence <= current {
			tx.commit().await?;
			return Ok(Outcome::Mirrored);
		}

		// AHEAD OF THE `match`, DELIBERATELY: the address is carried by EVERY kind, not by one.
		// An arm added later inherits the refresh instead of quietly not carrying it, which is
		// the exact shape of the bug this fixes (EV-invest/concierge#46).
		if freshness == Freshness::Live {
			refresh_profile_snapshot(&mut tx, subject, event).await?;
		}

		match event.kind() {
			// CREATED already upserted above; stamp the sequence, refresh KYC, and backfill
			// concierge_user_id if a pre-existing row didn't have it (COALESCE never overwrites).
			//
			// THIS ARM ALSO WRITES THE ROSTER JOURNAL, AND MUST KEEP DOING SO EVEN THOUGH NO
			// CREATED CARRIES `owner` TODAY. Concierge stamps the role snapshot at drain time,
			// and CREATED drains immediately after `User::provision`, when the role is still
			// `investor`. That is an ordering coincidence in ANOTHER repository, not an
			// invariant of this one. Were it to change, CREATED would be the only path by
			// which the money plane could gain an owner without a `governance_roster_change`
			// row, and therefore without the 48h cooling-off window the whole roster-capture
			// defence rests on (`application::consilium::ROSTER_COOLING_OFF_SECS`). Do not
			// delete this as dead code: it is a latch, and it costs one statement on a path
			// that runs once per user.
			//
			// Reached only when the row ALREADY existed (materialized by first sign-in) and a
			// later CREATED advances the sequence — a row this event inserted is handled
			// above, before the sequence guard returns early.
			Kind::Created => {
				// `FROM users old` reads the PRE-update snapshot, exactly as the RoleChanged
				// arm does, so both arms report the role they replaced the same way.
				let previous: Option<(uuid::Uuid, String)> = sqlx::query_as(
					"UPDATE users u SET kyc_level = $2, role = $3, last_lifecycle_sequence = $4, concierge_user_id = COALESCE(u.concierge_user_id, $5), updated_at = now() \
					 FROM users old WHERE u.auth_subject = $1 AND old.id = u.id RETURNING u.id, old.role",
				)
				.bind(subject)
				.bind(event.kyc_level as i32)
				.bind(role.as_str())
				.bind(sequence)
				.bind(concierge_user_id)
				.fetch_optional(&mut *tx)
				.await?;

				if let Some((user_id, from_role)) = previous {
					record_roster_change(&mut tx, user_id, &from_role, role.as_str()).await?;
				}
			}
			Kind::Suspended => {
				sqlx::query("UPDATE users SET frozen = TRUE, last_lifecycle_sequence = $2, updated_at = now() WHERE auth_subject = $1")
					.bind(subject)
					.bind(sequence)
					.execute(&mut *tx)
					.await?;
			}
			Kind::Reinstated => {
				sqlx::query("UPDATE users SET frozen = FALSE, last_lifecycle_sequence = $2, updated_at = now() WHERE auth_subject = $1")
					.bind(subject)
					.bind(sequence)
					.execute(&mut *tx)
					.await?;
			}
			Kind::KycChanged => {
				sqlx::query("UPDATE users SET kyc_level = $2, last_lifecycle_sequence = $3, updated_at = now() WHERE auth_subject = $1")
					.bind(subject)
					.bind(event.kyc_level as i32)
					.bind(sequence)
					.execute(&mut *tx)
					.await?;
			}
			Kind::RoleChanged => {
				// `FROM users old` reads the PRE-update snapshot, so one statement both
				// mirrors the new role and reports the one it replaced.
				let previous: Option<(uuid::Uuid, String)> = sqlx::query_as(
					"UPDATE users u SET role = $2, last_lifecycle_sequence = $3, updated_at = now() \
					 FROM users old WHERE u.auth_subject = $1 AND old.id = u.id RETURNING u.id, old.role",
				)
				.bind(subject)
				.bind(role.as_str())
				.bind(sequence)
				.fetch_optional(&mut *tx)
				.await?;

				if let Some((user_id, from_role)) = previous {
					record_roster_change(&mut tx, user_id, &from_role, role.as_str()).await?;
				}
			}
			Kind::SessionsRevoked => {
				// The revoke FLOOR only ratchets up — GREATEST guards against an out-of-order
				// lower value (the sequence guard already orders, this is belt-and-suspenders).
				sqlx::query("UPDATE users SET concierge_token_version = GREATEST(concierge_token_version, $2), last_lifecycle_sequence = $3, updated_at = now() WHERE auth_subject = $1")
					.bind(subject)
					.bind(event.token_version as i64)
					.bind(sequence)
					.execute(&mut *tx)
					.await?;
			}
			// Already refused at the top of this function, before any write. Repeated rather
			// than made `unreachable!()`: this arm decides whether a freeze or a tier
			// downgrade is silently marked applied, and it must fail closed even if some
			// future edit lets an unnameable kind reach it.
			Kind::Unspecified => {
				tx.rollback().await?;
				return Ok(Outcome::Unreadable);
			}
		}
		tx.commit().await?;
		Ok(Outcome::Mirrored)
	}
}

/// Mirror the address the event carries onto the local row.
///
/// THIS IS THE ONLY ROUTE AN EMAIL CHANGE HAS INTO THE MONEY PLANE. Concierge's
/// `change_email` bumps `row_version` WITHOUT `bump_and_emit`, on the stated contract that
/// "banking re-reads the email snapshot on the next lifecycle event" (`domain/src/users.rs`).
/// Banking never did: `email` was written once, by the CREATED insert, under
/// `ON CONFLICT DO NOTHING`, and no other arm touched it — so an address changed at the IdP
/// stayed at its CREATED value on this side forever (EV-invest/concierge#46). Since the
/// snapshot rides on every lifecycle row already, honouring it here costs no new event kind
/// and no coordinated rollout.
///
/// Two guards, both fail-closed:
///   - the value must PARSE as an `Email`. `users.email` is NOT NULL and every read
///     (`PgUsers::find_by_id`) parses it back, so an empty or malformed snapshot from an
///     older concierge would turn a loadable row into one whose profile and money surfaces
///     error out. A bad snapshot is dropped with a warning instead — the mirror keeps the
///     last address it trusted.
///   - the write is skipped when nothing differs, so the ordinary event (a freeze, a tier
///     move) does not churn `updated_at` for an address that never changed. `Email::parse`
///     normalizes exactly as `PgUsers::provision` does, so the two writers of this column
///     agree on spelling and cannot ping-pong it.
async fn refresh_profile_snapshot(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, subject: &str, event: &UserLifecycleEvent) -> Result<(), sqlx::Error> {
	let Ok(email) = Email::parse(&event.email) else {
		warn!(
			event_id = %event.event_id,
			subject = %subject,
			"bridge: lifecycle event carries no usable email snapshot — keeping the mirrored address"
		);
		return Ok(());
	};
	sqlx::query(
		"UPDATE users SET email = $2, email_verified = $3, updated_at = now() \
		 WHERE auth_subject = $1 AND (email IS DISTINCT FROM $2 OR email_verified IS DISTINCT FROM $3)",
	)
	.bind(subject)
	.bind(email.as_str())
	.bind(event.email_verified)
	.execute(&mut **tx)
	.await?;
	Ok(())
}

/// Park an event this build cannot apply yet. `ON CONFLICT DO NOTHING` keeps a redelivery —
/// or an event re-parked by `replay_deferred` — from disturbing the original `deferred_at`,
/// which is the only signal an operator has for how long a subject has been stuck.
async fn defer(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, event: &UserLifecycleEvent, concierge_user_id: Option<uuid::Uuid>) -> Result<(), sqlx::Error> {
	sqlx::query(
		"INSERT INTO bridge_deferred_event (event_id, auth_subject, kind, sequence, concierge_user_id, email, email_verified, kyc_level, role, token_version, occurred_at) \
		 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) ON CONFLICT (event_id) DO NOTHING",
	)
	.bind(&event.event_id)
	.bind(&event.auth_subject)
	.bind(event.kind)
	.bind(event.sequence as i64)
	.bind(concierge_user_id)
	.bind(&event.email)
	.bind(event.email_verified)
	.bind(event.kyc_level as i32)
	.bind(&event.role)
	.bind(event.token_version as i64)
	.bind(event.occurred_at)
	.execute(&mut **tx)
	.await?;
	warn!(
		event_id = %event.event_id,
		subject = %event.auth_subject,
		kind = event.kind,
		sequence = event.sequence,
		"bridge: subject not provisioned locally — parking the lifecycle event for replay"
	);
	Ok(())
}

/// A parked event, read back column-for-column. EVERY FIELD `apply` READS OFF A
/// `UserLifecycleEvent` MUST BE HERE and in `bridge_deferred_event`: what this struct
/// doesn't carry, a replayed event doesn't carry either, and the loss is silent.
#[derive(sqlx::FromRow)]
struct DeferredEvent {
	event_id: String,
	auth_subject: String,
	kind: i32,
	sequence: i64,
	concierge_user_id: Option<uuid::Uuid>,
	email: String,
	email_verified: bool,
	kyc_level: i32,
	role: String,
	token_version: i64,
	occurred_at: i64,
}

impl DeferredEvent {
	fn into_event(self) -> UserLifecycleEvent {
		UserLifecycleEvent {
			// An unparsable concierge id was stored as NULL on the way in; an empty string on
			// the way out parses back to `None`, which is what the apply path already expects.
			user_id: self.concierge_user_id.map(|id| id.to_string()).unwrap_or_default(),
			kind: self.kind,
			kyc_level: self.kyc_level as u32,
			occurred_at: self.occurred_at,
			event_id: self.event_id,
			sequence: self.sequence as u64,
			auth_subject: self.auth_subject,
			email: self.email,
			email_verified: self.email_verified,
			token_version: self.token_version as u64,
			role: self.role,
		}
	}
}

/// A CHANGE TO THE VOTING ROSTER STARTS A COOLING-OFF PERIOD.
///
/// Only owner-affecting transitions are recorded: an investor promoted to admin, or a KYC
/// level moving, does not change who can authorize a payout and must not delay a
/// legitimate one. A no-op transition is likewise not a change. Both the table's CHECK
/// constraints encode exactly this, so a caller that gets the predicate wrong fails the
/// insert rather than silently widening the window.
///
/// Called from EVERY lifecycle arm that can write `users.role`, so the money plane has no
/// route to gaining or losing an owner that skips the journal — see
/// `application::consilium::ROSTER_COOLING_OFF_SECS`. It takes the caller's transaction so
/// the roster and the clock measuring it commit together and can never disagree.
async fn record_roster_change(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, user_id: uuid::Uuid, from_role: &str, to_role: &str) -> Result<(), sqlx::Error> {
	if from_role == to_role || (from_role != Role::Owner.as_str() && to_role != Role::Owner.as_str()) {
		return Ok(());
	}
	sqlx::query("INSERT INTO governance_roster_change (user_id, from_role, to_role) VALUES ($1, $2, $3)")
		.bind(user_id)
		.bind(from_role)
		.bind(to_role)
		.execute(&mut **tx)
		.await?;
	warn!(%user_id, %from_role, %to_role, "governance: the owner roster changed — payout proposals are frozen for the cooling-off period");
	Ok(())
}

/// Whether the caller's banking row is blocked from moving money — the money-op gate.
/// Blocked by EITHER a concierge SUSPENDED (mirrored into `frozen`) OR a banking-side
/// DisableUser (`status='disabled'`), the SAME fold issuance/refresh already apply
/// (`resolve_issuance_by_*`); otherwise a banking DisableUser would not stop
/// subscribe/redeem during the access-token TTL, unlike a concierge SUSPENDED.
/// `None` (no local row) reads as BLOCKED. Every caller reaches this with a minted money
/// token, and minting resolves the same row (`resolve_issuance_by_*`), so a missing row is
/// not the ordinary "not provisioned yet" case — it is a state the gate cannot evaluate,
/// on a path whose next step moves money. Errs to the caller as a control-plane failure
/// (mapped to UNAVAILABLE) — fail-closed, like every other arm here.
pub async fn is_frozen(pool: &PgPool, user_id: UserId) -> Result<bool, sqlx::Error> {
	let blocked: Option<bool> = sqlx::query_scalar("SELECT (frozen OR status = 'disabled') FROM users WHERE id = $1")
		.bind(user_id.raw())
		.fetch_optional(pool)
		.await?;
	Ok(blocked.unwrap_or(true))
}

/// The mirrored access role for a banking user id (the money-op RBAC gate reads this).
/// `None` local row ⇒ `Investor` (holds nothing) so the gate fails closed. A corrupt
/// stored value likewise degrades to `Investor` rather than erroring the gate open.
pub async fn role_of(pool: &PgPool, user_id: UserId) -> Result<Role, sqlx::Error> {
	let role: Option<String> = sqlx::query_scalar("SELECT role FROM users WHERE id = $1").bind(user_id.raw()).fetch_optional(pool).await?;
	Ok(role.as_deref().map(Role::parse_or_default).unwrap_or_default())
}
