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
	async fn drain(&self, client: &mut UserEventsClient<Channel>) -> anyhow::Result<()> {
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
			sqlx::query(
				"INSERT INTO users (id, auth_subject, concierge_user_id, email, email_verified, kyc_level, role, last_lifecycle_sequence) \
				 VALUES (gen_random_uuid(), $1, $2, $3, $4, $5, $6, $7) ON CONFLICT (auth_subject) DO NOTHING",
			)
			.bind(subject)
			.bind(concierge_user_id)
			.bind(&event.email)
			.bind(event.email_verified)
			.bind(event.kyc_level as i32)
			.bind(role.as_str())
			.bind(sequence)
			.execute(&mut *tx)
			.await?;
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
			Kind::Created => {
				sqlx::query(
					"UPDATE users SET kyc_level = $2, role = $3, last_lifecycle_sequence = $4, concierge_user_id = COALESCE(concierge_user_id, $5), updated_at = now() WHERE auth_subject = $1",
				)
				.bind(subject)
				.bind(event.kyc_level as i32)
				.bind(role.as_str())
				.bind(sequence)
				.bind(concierge_user_id)
				.execute(&mut *tx)
				.await?;
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
				sqlx::query("UPDATE users SET role = $2, last_lifecycle_sequence = $3, updated_at = now() WHERE auth_subject = $1")
					.bind(subject)
					.bind(role.as_str())
					.bind(sequence)
					.execute(&mut *tx)
					.await?;
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
