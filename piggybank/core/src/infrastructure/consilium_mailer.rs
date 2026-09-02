//! Consilium mail worker — drains `consilium_mail` into the identity plane's mailer.
//!
//! WHY THIS IS A WORKER AND NOT AN INLINE CALL. Every governance mail is written in the same
//! transaction as the fact it announces. A vote that was validly cast must not be rolled
//! back because concierge was briefly unreachable, and a vote must not be recorded with its
//! notification silently dropped. Committing both together and delivering the second half
//! afterwards is the only shape that gives both — the same argument the outbox relay makes
//! for money.
//!
//! The relay RPC is idempotent by `dedupe_key`, so redelivering is free. On success the
//! payload is rewritten **without** its secrets: the plaintext approval code exists for
//! exactly as long as it takes to hand the message over, and no longer.

use std::{sync::Arc, time::Duration};

use domain::error::DomainError;
use sqlx::{PgPool, Row};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::ports::governance_mail::{GovernanceMail, GovernanceMailer};

const SWEEP_INTERVAL: Duration = Duration::from_secs(30);

/// Rows per pass. Governance mail is low volume by nature — a handful per consilium.
const BATCH: i64 = 100;

/// After this many failures a row stops being retried and starts being an alert. It is
/// never deleted: the audit trail keeps what could not be delivered, and why.
const MAX_ATTEMPTS: i32 = 10;

pub struct ConsiliumMailer {
	pool: PgPool,
	mailer: Arc<dyn GovernanceMailer>,
}

impl ConsiliumMailer {
	pub fn new(pool: PgPool, mailer: Arc<dyn GovernanceMailer>) -> Self {
		Self { pool, mailer }
	}

	/// Run as the SINGLETON mailer.
	///
	/// The relay RPC is idempotent by `dedupe_key`, so a second mailer cannot double-send —
	/// but it can double the work, double the `attempts` charged against a struggling
	/// recipient, and walk a row to `MAX_ATTEMPTS` twice as fast during an outage, turning a
	/// recoverable concierge blip into a permanently abandoned approval mail. Multi-replica
	/// is an anticipated shape here, so the singleton is enforced rather than assumed.
	///
	/// As in the relay and the sweeper, a transient DB failure re-acquires with backoff and
	/// never returns — an early return would cancel every sibling task in the composition
	/// root's `join!`.
	pub async fn run(self, shutdown: CancellationToken) {
		const MAX_BACKOFF: Duration = Duration::from_secs(30);
		let mut backoff = Duration::from_millis(500);
		info!("consilium mailer: delivering governance mail every {SWEEP_INTERVAL:?}");
		'acquire: loop {
			let mut lock = tokio::select! {
				biased;
				() = shutdown.cancelled() => return,
				lock = super::singleton::acquire(&self.pool, super::singleton::keys::CONSILIUM_MAILER) => match lock {
					Ok(lock) => lock,
					Err(err) => {
						error!("consilium mailer: could not acquire the mailer lock (retrying in {backoff:?}): {err}");
						tokio::select! {
							() = shutdown.cancelled() => return,
							() = tokio::time::sleep(backoff) => {},
						}
						backoff = (backoff * 2).min(MAX_BACKOFF);
						continue 'acquire;
					}
				},
			};
			backoff = Duration::from_millis(500);
			info!("consilium mailer: acquired the mailer lock — delivering as the singleton worker");
			loop {
				if let Err(err) = self.drain().await {
					warn!("consilium mailer: drain failed (will retry): {err}");
				}
				tokio::select! {
					() = shutdown.cancelled() => return,
					() = tokio::time::sleep(SWEEP_INTERVAL) => {},
				}
				if !super::singleton::still_held(&mut lock).await {
					error!("consilium mailer: lost the mailer lock connection — re-acquiring");
					let _ = lock.close().await;
					continue 'acquire;
				}
			}
		}
	}

	/// One pass. Returns how many messages were handed over. Public so an integration test
	/// can drive it deterministically.
	pub async fn drain(&self) -> Result<usize, DomainError> {
		let rows = sqlx::query(
			"SELECT m.id, m.dedupe_key, m.payload::text AS payload, u.concierge_user_id \
			 FROM consilium_mail m JOIN users u ON u.id = m.user_id \
			 WHERE m.sent_at IS NULL AND m.attempts < $1 ORDER BY m.id LIMIT $2",
		)
		.bind(MAX_ATTEMPTS)
		.bind(BATCH)
		.fetch_all(&self.pool)
		.await
		.map_err(repo_err)?;

		let mut sent = 0;
		for row in &rows {
			let id: i64 = row.try_get("id").map_err(repo_err)?;
			let dedupe_key: String = row.try_get("dedupe_key").map_err(repo_err)?;
			let payload: String = row.try_get("payload").map_err(repo_err)?;
			let concierge_user_id: Option<Uuid> = row.try_get("concierge_user_id").map_err(repo_err)?;
			let mail: GovernanceMail = match serde_json::from_str(&payload) {
				Ok(mail) => mail,
				Err(err) => {
					self.fail(id, &format!("unreadable payload: {err}")).await?;
					continue;
				}
			};
			// The recipient is addressed in the plane that OWNS identities, so the money
			// plane cannot redirect a governance mail. Without the mirrored id there is no
			// safe address to send to, and guessing is not an option.
			let Some(recipient) = concierge_user_id else {
				self.fail(id, "recipient has no mirrored concierge user id").await?;
				continue;
			};
			match self.mailer.send(recipient, &dedupe_key, &mail).await {
				Ok(()) => {
					let redacted = serde_json::to_string(&mail.redacted()).map_err(|e| DomainError::Repository(e.to_string()))?;
					sqlx::query("UPDATE consilium_mail SET sent_at = now(), payload = $2::jsonb WHERE id = $1")
						.bind(id)
						.bind(redacted)
						.execute(&self.pool)
						.await
						.map_err(repo_err)?;
					// `consilium_voter.notified` used to be set TRUE at INSERT, where it meant
					// "queued" while the operator screen reading it says "notified". During a
					// concierge outage every seat would show as notified with not one mail
					// delivered — the screen would look healthiest exactly when the mechanism
					// was most broken. It is set HERE, once concierge has actually taken the
					// message, and only for the approval mail: an outcome or burn notice tells
					// an owner nothing about whether they were given a token to vote with.
					if matches!(mail, GovernanceMail::PayoutApproval(_)) {
						sqlx::query("UPDATE consilium_voter SET notified = TRUE WHERE consilium_id = (SELECT consilium_id FROM consilium_mail WHERE id = $1) AND user_id = (SELECT user_id FROM consilium_mail WHERE id = $1)")
							.bind(id)
							.execute(&self.pool)
							.await
							.map_err(repo_err)?;
					}
					sent += 1;
				}
				Err(err) => self.fail(id, &err.to_string()).await?,
			}
		}
		Ok(sent)
	}

	/// Record a delivery failure. At the ceiling it becomes an `error!` (Sentry-shipped): an
	/// approval mail that never arrives is a consilium that can never reach quorum, which is
	/// exactly the kind of silent stall an operator must be told about.
	async fn fail(&self, id: i64, reason: &str) -> Result<(), DomainError> {
		let attempts: i32 = sqlx::query_scalar("UPDATE consilium_mail SET attempts = attempts + 1, last_error = $2 WHERE id = $1 RETURNING attempts")
			.bind(id)
			.bind(reason)
			.fetch_one(&self.pool)
			.await
			.map_err(repo_err)?;
		if attempts >= MAX_ATTEMPTS {
			error!(mail_id = id, attempts, "consilium mailer: giving up on a governance mail — an owner will not be told: {reason}");
		} else {
			warn!(mail_id = id, attempts, "consilium mailer: delivery failed (will retry): {reason}");
		}
		Ok(())
	}
}

fn repo_err(err: sqlx::Error) -> DomainError {
	DomainError::Repository(err.to_string())
}

/// How many governance mails are still undelivered — the number the boot warning quotes when
/// the seam is unwired.
pub async fn pending_count(pool: &PgPool) -> Result<i64, DomainError> {
	sqlx::query_scalar("SELECT COUNT(*) FROM consilium_mail WHERE sent_at IS NULL")
		.fetch_one(pool)
		.await
		.map_err(repo_err)
}
