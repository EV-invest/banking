//! Postgres adapter for the [`UserRepository`] port (the control plane).
//!
//! Each mutating method opens one transaction, writes the user row, and appends
//! the aggregate's drained events to the `event_log` in that same transaction —
//! the single ACID point. Runtime queries (`sqlx::query*`, not the compile-time
//! macros) keep `cargo build` independent of a live database.

use async_trait::async_trait;
use domain::{
	architecture::{AggregateRoot, DomainEvent, EmitsEvents, Reader, Repository},
	auth::AuthSubject,
	error::DomainError,
	users::{Email, User, UserEvent, UserId, UserStatus},
};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::ports::UserRepository;

pub struct PgUsers {
	pool: PgPool,
}

impl PgUsers {
	pub fn new(pool: PgPool) -> Self {
		Self { pool }
	}
}

impl Repository for PgUsers {
	type Aggregate = User;
}

impl Reader for PgUsers {
	type Aggregate = User;
}

#[derive(sqlx::FromRow)]
struct UserRow {
	id: Uuid,
	auth_subject: String,
	email: String,
	email_verified: bool,
	status: String,
	token_version: i64,
}

impl UserRow {
	fn into_domain(self) -> Result<User, DomainError> {
		Ok(User::rehydrate(
			UserId::from_raw(self.id),
			AuthSubject::parse(&self.auth_subject)?,
			Email::parse(&self.email)?,
			self.email_verified,
			UserStatus::parse(&self.status)?,
			self.token_version as u64,
		))
	}
}

fn repo_err(err: sqlx::Error) -> DomainError {
	DomainError::Repository(err.to_string())
}

#[async_trait]
impl UserRepository for PgUsers {
	async fn find_by_id(&self, id: UserId) -> Result<Option<User>, DomainError> {
		let row = sqlx::query_as::<_, UserRow>("SELECT id, auth_subject, email, email_verified, status, token_version FROM users WHERE id = $1")
			.bind(id.raw())
			.fetch_optional(&self.pool)
			.await
			.map_err(repo_err)?;
		row.map(UserRow::into_domain).transpose()
	}

	async fn provision(&self, subject: AuthSubject, email: Email, email_verified: bool) -> Result<User, DomainError> {
		let mut tx = self.pool.begin().await.map_err(repo_err)?;

		let existing = sqlx::query_as::<_, UserRow>("SELECT id, auth_subject, email, email_verified, status, token_version FROM users WHERE auth_subject = $1 FOR UPDATE")
			.bind(subject.as_str())
			.fetch_optional(&mut *tx)
			.await
			.map_err(repo_err)?;

		let mut user = match existing {
			Some(row) => update_email_on(&mut tx, row, email, email_verified).await?,
			None => {
				// First sign-in for this subject. `FOR UPDATE` above locked no row (none
				// existed), so a concurrent first-login could be inserting too — hence
				// `ON CONFLICT DO NOTHING`. If we win, the aggregate (with its
				// `Provisioned` event) is ours; if we lose, re-read the row the other
				// transaction created and take the email-update path. Idempotent either way.
				let candidate = User::provision(UserId::new(), subject.clone(), email.clone(), email_verified);
				let inserted = sqlx::query_scalar::<_, Uuid>(
					"INSERT INTO users (id, auth_subject, email, email_verified, status, token_version) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (auth_subject) DO NOTHING RETURNING id",
				)
				.bind(candidate.id().raw())
				.bind(candidate.auth_subject().as_str())
				.bind(candidate.email().as_str())
				.bind(candidate.email_verified())
				.bind(candidate.status().as_str())
				.bind(candidate.token_version() as i64)
				.fetch_optional(&mut *tx)
				.await
				.map_err(repo_err)?;

				match inserted {
					Some(_) => candidate,
					None => {
						let row = sqlx::query_as::<_, UserRow>("SELECT id, auth_subject, email, email_verified, status, token_version FROM users WHERE auth_subject = $1 FOR UPDATE")
							.bind(subject.as_str())
							.fetch_one(&mut *tx)
							.await
							.map_err(repo_err)?;
						update_email_on(&mut tx, row, email, email_verified).await?
					}
				}
			}
		};

		append_events(&mut tx, &mut user).await?;
		tx.commit().await.map_err(repo_err)?;
		Ok(user)
	}

	async fn save(&self, user: &mut User) -> Result<(), DomainError> {
		let mut tx = self.pool.begin().await.map_err(repo_err)?;
		sqlx::query("UPDATE users SET email = $2, email_verified = $3, status = $4, token_version = $5, updated_at = now() WHERE id = $1")
			.bind(user.id().raw())
			.bind(user.email().as_str())
			.bind(user.email_verified())
			.bind(user.status().as_str())
			.bind(user.token_version() as i64)
			.execute(&mut *tx)
			.await
			.map_err(repo_err)?;
		append_events(&mut tx, user).await?;
		tx.commit().await.map_err(repo_err)?;
		Ok(())
	}
}

/// Apply the IdP's current email to an existing row (raising `EmailChanged` only if
/// it differs) and persist the column. Shared by the "user already exists" and
/// "lost the first-login race" paths.
async fn update_email_on(conn: &mut PgConnection, row: UserRow, email: Email, email_verified: bool) -> Result<User, DomainError> {
	let mut user = row.into_domain()?;
	user.change_email(email, email_verified);
	sqlx::query("UPDATE users SET email = $2, email_verified = $3, updated_at = now() WHERE id = $1")
		.bind(user.id().raw())
		.bind(user.email().as_str())
		.bind(user.email_verified())
		.execute(&mut *conn)
		.await
		.map_err(repo_err)?;
	Ok(user)
}

/// Drain the aggregate's pending events into the `event_log` on the same connection
/// (the open transaction), so state and events commit together or not at all.
async fn append_events(conn: &mut PgConnection, user: &mut User) -> Result<(), DomainError> {
	for event in user.drain_events() {
		let payload = serde_json::to_string(&event).map_err(|e| DomainError::Repository(e.to_string()))?;
		sqlx::query("INSERT INTO event_log (aggregate, aggregate_id, kind, payload) VALUES ($1, $2, $3, $4::jsonb)")
			.bind(User::NAME)
			.bind(user.id().raw())
			.bind(UserEvent::KIND)
			.bind(payload)
			.execute(&mut *conn)
			.await
			.map_err(repo_err)?;
	}
	Ok(())
}
