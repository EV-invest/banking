//! Integration tests for the Postgres `UserRepository` adapter.
//!
//! These hit a **real** Postgres (no mocks, per the project rules). They run when
//! `DATABASE_URL` is set (e.g. after `nix run .#db`) and skip otherwise, so a
//! DB-less `cargo test` still passes. Each test uses a fresh random `auth_subject`,
//! so runs neither collide nor require a clean database.

use domain::{
	auth::AuthSubject,
	users::{Email, UserStatus},
};
use piggybank_core::{
	infrastructure::{db, users::PgUsers},
	ports::UserRepository,
};

async fn repo() -> Option<PgUsers> {
	let url = std::env::var("DATABASE_URL").ok().filter(|s| !s.is_empty())?;
	let pool = db::connect(&url).await.expect("connect to Postgres");
	db::migrate(&pool).await.expect("apply migrations");
	Some(PgUsers::new(pool))
}

fn unique_subject() -> AuthSubject {
	AuthSubject::parse(&format!("itest-{}", uuid::Uuid::new_v4())).unwrap()
}

#[tokio::test]
async fn provision_is_idempotent_by_subject() {
	let Some(repo) = repo().await else {
		eprintln!("DATABASE_URL unset — skipping real-DB test");
		return;
	};
	let subject = unique_subject();
	let email = Email::parse("itest@example.com").unwrap();

	let first = repo.provision(subject.clone(), email.clone(), true).await.unwrap();
	let again = repo.provision(subject.clone(), email.clone(), true).await.unwrap();

	assert_eq!(first.id(), again.id(), "one subject must map to exactly one user");
	assert_eq!(again.token_version(), 0);
	assert!(again.is_active());
}

#[tokio::test]
async fn provision_is_idempotent_under_concurrency() {
	let Some(repo) = repo().await else {
		return;
	};
	let subject = unique_subject();
	let email = Email::parse("race@example.com").unwrap();

	// Two concurrent first-logins for the same subject must both succeed and converge
	// on one user (the ON CONFLICT upsert path), not fail with a unique violation.
	// `join!` drives both futures on one task — they interleave at their awaits and
	// hit the insert race without a detached spawn.
	let (a, b) = tokio::join!(repo.provision(subject.clone(), email.clone(), true), repo.provision(subject.clone(), email.clone(), true),);
	let a = a.expect("first concurrent provision");
	let b = b.expect("second concurrent provision");
	assert_eq!(a.id(), b.id(), "concurrent first-logins must converge on one user");
}

#[tokio::test]
async fn reprovision_updates_email() {
	let Some(repo) = repo().await else {
		return;
	};
	let subject = unique_subject();
	let created = repo.provision(subject.clone(), Email::parse("before@example.com").unwrap(), true).await.unwrap();
	let updated = repo.provision(subject.clone(), Email::parse("After@Example.com").unwrap(), true).await.unwrap();

	assert_eq!(created.id(), updated.id());
	assert_eq!(updated.email().as_str(), "after@example.com", "email is updated and normalized");
}

#[tokio::test]
async fn revoke_and_disable_persist() {
	let Some(repo) = repo().await else {
		return;
	};
	let user = repo.provision(unique_subject(), Email::parse("rev@example.com").unwrap(), true).await.unwrap();

	let revoked = repo.revoke_tokens(user.id()).await.unwrap();
	assert_eq!(revoked.token_version(), 1);

	let loaded = repo.find_by_id(user.id()).await.unwrap().unwrap();
	assert_eq!(loaded.token_version(), 1);
	repo.disable(user.id()).await.unwrap();

	let after = repo.find_by_id(user.id()).await.unwrap().unwrap();
	assert_eq!(after.status(), UserStatus::Disabled);
	assert_eq!(after.token_version(), 1);
}
