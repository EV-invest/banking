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
//! Correlation is by `auth_subject` (the provider `sub` both planes provision against),
//! never concierge's own `user_id` — a CREATED event provisions a minimal local row for an
//! as-yet-unseen subject (banking otherwise materializes a user on first sign-in).

use std::time::Duration;

use domain::{authz::Role, users::UserId};
use evconcierge_contracts::concierge::v1::{PullUserLifecycleRequest, UserLifecycleEvent, user_events_client::UserEventsClient, user_lifecycle_event::Kind};
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use tonic::{Request, metadata::MetadataValue, transport::Channel};
use tracing::{info, warn};

/// How many outbox rows to request per pull. The server caps `limit` at its own ceiling
/// (500), so this is the steady-state batch, not a hard bound.
const PULL_LIMIT: u32 = 256;

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
	async fn drain(&self, client: &mut UserEventsClient<Channel>) -> color_eyre::Result<()> {
		loop {
			let after = self.cursor().await?;
			let mut request = Request::new(PullUserLifecycleRequest {
				after_position: after,
				limit: PULL_LIMIT,
			});
			let token: MetadataValue<_> = format!("Bearer {}", self.service_token).parse()?;
			request.metadata_mut().insert("authorization", token);

			let response = client.pull_user_lifecycle(request).await?.into_inner();
			if response.events.is_empty() {
				return Ok(());
			}
			for event in &response.events {
				self.apply(event).await?;
			}
			self.advance_cursor(after, response.next_position).await?;
			// A short batch (server gave back fewer than it caps) means we caught up.
			if (response.events.len() as u32) < PULL_LIMIT {
				return Ok(());
			}
		}
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
	/// new sequence. CREATED provisions a minimal row for an unseen subject. An unknown/
	/// unspecified `kind` is a benign no-op (forward-compat with a newer concierge enum).
	async fn apply(&self, event: &UserLifecycleEvent) -> Result<(), sqlx::Error> {
		let subject = &event.auth_subject;
		let sequence = event.sequence as i64;
		// Concierge's own user id — the handle the BFF later presents on IssueUserToken. A
		// malformed value (should never happen) is stored as NULL rather than failing the event.
		let concierge_user_id = uuid::Uuid::parse_str(&event.user_id).ok();
		let mut tx = self.pool.begin().await?;

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
			// being built by another path) — nothing to mutate. The eventual CREATED/sign-in
			// materializes it; redelivery then catches up. Don't advance anything.
			tx.commit().await?;
			return Ok(());
		};
		if sequence <= current {
			tx.commit().await?;
			return Ok(());
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
			Kind::Unspecified => {
				// Forward-compat: a newer concierge kind this build doesn't know. Advance the
				// per-user guard so it isn't re-fetched forever, but mutate nothing.
				sqlx::query("UPDATE users SET last_lifecycle_sequence = $2, updated_at = now() WHERE auth_subject = $1")
					.bind(subject)
					.bind(sequence)
					.execute(&mut *tx)
					.await?;
			}
		}
		tx.commit().await?;
		Ok(())
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
/// `None` (no local row yet) is NOT blocked: a user with no row has nothing to move, and
/// the downstream solvency checks gate that case anyway. Errs to the caller as a
/// control-plane failure (mapped to UNAVAILABLE) — fail-closed when the gate can't be read.
pub async fn is_frozen(pool: &PgPool, user_id: UserId) -> Result<bool, sqlx::Error> {
	let blocked: Option<bool> = sqlx::query_scalar("SELECT (frozen OR status = 'disabled') FROM users WHERE id = $1")
		.bind(user_id.raw())
		.fetch_optional(pool)
		.await?;
	Ok(blocked.unwrap_or(false))
}

/// The mirrored access role for a banking user id (the money-op RBAC gate reads this).
/// `None` local row ⇒ `Investor` (holds nothing) so the gate fails closed. A corrupt
/// stored value likewise degrades to `Investor` rather than erroring the gate open.
pub async fn role_of(pool: &PgPool, user_id: UserId) -> Result<Role, sqlx::Error> {
	let role: Option<String> = sqlx::query_scalar("SELECT role FROM users WHERE id = $1").bind(user_id.raw()).fetch_optional(pool).await?;
	Ok(role.as_deref().map(Role::parse_or_default).unwrap_or_default())
}
