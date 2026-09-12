//! The mail worker against a real queue, with the relay stubbed at its port — real Postgres,
//! no DB mocks (project rule); the identity plane's relay is the one external gateway here,
//! stood in for the way `StubCustody` stands in for the signer.
//!
//! What is pinned: a relay that THROTTLES a recipient (`RESOURCE_EXHAUSTED`) or is down
//! defers the mail without spending one of its attempts, the deferred row waits out its
//! backoff before the relay is asked again, an actual refusal still costs an attempt, and a
//! mail deferred for longer than the ceiling is finally given up on.

use std::sync::{
	Arc,
	atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use domain::{
	auth::AuthSubject,
	users::{Email, UserId},
};
use piggybank_core::{
	infrastructure::{consilium_mailer::ConsiliumMailer, users::PgUsers},
	ports::{
		UserRepository,
		governance_mail::{GovernanceMail, GovernanceMailer, MailDeliveryError, PayoutOutcome},
	},
};
use sqlx::PgPool;
use uuid::Uuid;

mod common;

/// The queue is one table shared with the governance suites, whose fixtures leave undelivered
/// rows behind; the worker drains in batches of 100 by id, so a backlog would push this
/// suite's rows out of the batch. Every test runs alone and starts by retiring the backlog.
static QUEUE: std::sync::LazyLock<tokio::sync::Mutex<()>> = std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

async fn quiet_queue(pool: &PgPool) {
	sqlx::query("UPDATE consilium_mail SET sent_at = now() WHERE sent_at IS NULL").execute(pool).await.unwrap();
}

/// A relay that answers every send the same way and counts how often it was asked.
struct FixedRelay {
	answer: fn() -> Result<(), MailDeliveryError>,
	calls: AtomicUsize,
}

#[async_trait]
impl GovernanceMailer for FixedRelay {
	async fn send(&self, _recipient: Uuid, _dedupe_key: &str, _mail: &GovernanceMail) -> Result<(), MailDeliveryError> {
		self.calls.fetch_add(1, Ordering::SeqCst);
		(self.answer)()
	}
}

fn throttling() -> Arc<FixedRelay> {
	Arc::new(FixedRelay {
		answer: || Err(MailDeliveryError::Deferred("governance mail relay: status: ResourceExhausted".into())),
		calls: AtomicUsize::new(0),
	})
}

fn refusing() -> Arc<FixedRelay> {
	Arc::new(FixedRelay {
		answer: || Err(MailDeliveryError::Failed("governance mail relay: status: InvalidArgument".into())),
		calls: AtomicUsize::new(0),
	})
}

/// A recipient with a mirrored concierge id, a terminal consilium fixture to hang the mail
/// on, and one queued outcome mail. Returns the mail row's id.
async fn a_queued_mail(pool: &PgPool) -> i64 {
	let users = PgUsers::new(pool.clone());
	let tag = Uuid::new_v4();
	let owner: UserId = users
		.provision(
			AuthSubject::parse(&format!("mailer-{tag}")).unwrap(),
			Email::parse(&format!("mailer-{tag}@example.test")).unwrap(),
			true,
		)
		.await
		.unwrap()
		.id();
	sqlx::query("UPDATE users SET concierge_user_id = $2 WHERE id = $1")
		.bind(owner.raw())
		.bind(Uuid::new_v4())
		.execute(pool)
		.await
		.unwrap();
	let consilium = Uuid::new_v4();
	sqlx::query(
		"INSERT INTO consilium (id, kind, state, terms, source_claim, payload_hash, initiator_user_id, owner_count, threshold, expires_at, decided_at) \
		 VALUES ($1, 'revenue_payout', 'cancelled', $2::jsonb, 'fee', $3, $4, 3, 2, now() + interval '72 hours', now())",
	)
	.bind(consilium)
	.bind(r#"{"network":"bep20","address":"0x52908400098527886E0F7030069857D2E4169EE7","amount":"1","memo":"fixture"}"#)
	.bind(vec![9u8; 32])
	.bind(owner.raw())
	.execute(pool)
	.await
	.unwrap();
	let mail = GovernanceMail::PayoutOutcome(PayoutOutcome {
		consilium_id: consilium.to_string(),
		outcome: "CANCELLED".into(),
		network: "bep20".into(),
		address: "0x52908400098527886E0F7030069857D2E4169EE7".into(),
		amount: "1".into(),
		detail: "fixture".into(),
		tier: String::new(),
		source: String::new(),
		destination: String::new(),
		reason: String::new(),
	});
	sqlx::query_scalar("INSERT INTO consilium_mail (consilium_id, user_id, kind, dedupe_key, payload) VALUES ($1, $2, $3, $4, $5::jsonb) RETURNING id")
		.bind(consilium)
		.bind(owner.raw())
		.bind(mail.as_str())
		.bind(format!("mailer-test:{tag}"))
		.bind(serde_json::to_string(&mail).unwrap())
		.fetch_one(pool)
		.await
		.unwrap()
}

async fn row(pool: &PgPool, id: i64) -> (i32, i32, bool, bool) {
	sqlx::query_as("SELECT attempts, deferrals, next_attempt_at IS NOT NULL AND next_attempt_at > now(), sent_at IS NOT NULL FROM consilium_mail WHERE id = $1")
		.bind(id)
		.fetch_one(pool)
		.await
		.unwrap()
}

#[tokio::test]
async fn a_throttled_recipient_is_deferred_with_backoff_and_charged_no_attempt() {
	let _guard = QUEUE.lock().await;
	let Some(pool) = common::pool().await else {
		eprintln!("DATABASE_URL unset — skipping the mailer suite");
		return;
	};
	quiet_queue(&pool).await;
	let id = a_queued_mail(&pool).await;
	let relay = throttling();
	let mailer = ConsiliumMailer::new(pool.clone(), relay.clone());

	assert_eq!(mailer.drain().await.unwrap(), 0);
	assert_eq!(relay.calls.load(Ordering::SeqCst), 1);
	let (attempts, deferrals, waiting, sent) = row(&pool, id).await;
	assert_eq!((attempts, deferrals, waiting, sent), (0, 1, true, false), "deferred: nothing charged, a wait recorded");

	// While the backoff runs the relay is not asked again — the whole point of backing off
	// from a limit that is protecting the recipient.
	assert_eq!(mailer.drain().await.unwrap(), 0);
	assert_eq!(relay.calls.load(Ordering::SeqCst), 1, "a deferred row is left alone until its wait is over");

	// The wait runs out: asked again, deferred again, the wait grows.
	sqlx::query("UPDATE consilium_mail SET next_attempt_at = now() - interval '1 second' WHERE id = $1")
		.bind(id)
		.execute(&pool)
		.await
		.unwrap();
	mailer.drain().await.unwrap();
	assert_eq!(relay.calls.load(Ordering::SeqCst), 2);
	let (attempts, deferrals, waiting, _) = row(&pool, id).await;
	assert_eq!((attempts, deferrals, waiting), (0, 2, true));
	let backoff_seconds: f64 = sqlx::query_scalar("SELECT EXTRACT(EPOCH FROM next_attempt_at - now())::float8 FROM consilium_mail WHERE id = $1")
		.bind(id)
		.fetch_one(&pool)
		.await
		.unwrap();
	assert!(backoff_seconds > 45.0, "the second deferral waits longer than the first: {backoff_seconds}s");

	// A GENUINE refusal, by contrast, still costs an attempt.
	sqlx::query("UPDATE consilium_mail SET next_attempt_at = NULL WHERE id = $1")
		.bind(id)
		.execute(&pool)
		.await
		.unwrap();
	ConsiliumMailer::new(pool.clone(), refusing()).drain().await.unwrap();
	let (attempts, deferrals, _, _) = row(&pool, id).await;
	assert_eq!((attempts, deferrals), (1, 2), "a refusal is charged; the deferrals are not forgiven");

	sqlx::query("DELETE FROM consilium_mail WHERE id = $1").bind(id).execute(&pool).await.unwrap();
}

#[tokio::test]
async fn a_mail_deferred_for_longer_than_the_ceiling_is_given_up_on() {
	let _guard = QUEUE.lock().await;
	let Some(pool) = common::pool().await else {
		eprintln!("DATABASE_URL unset — skipping the mailer suite");
		return;
	};
	quiet_queue(&pool).await;
	let id = a_queued_mail(&pool).await;
	// Queued more than a day ago and throttled ever since.
	sqlx::query("UPDATE consilium_mail SET created_at = now() - interval '25 hours' WHERE id = $1")
		.bind(id)
		.execute(&pool)
		.await
		.unwrap();
	let relay = throttling();
	ConsiliumMailer::new(pool.clone(), relay.clone()).drain().await.unwrap();
	let (attempts, _, _, sent) = row(&pool, id).await;
	assert!(attempts >= 10 && !sent, "past the ceiling the row is retired the way a failed one is: attempts={attempts}");
	// And never asked about again.
	ConsiliumMailer::new(pool.clone(), relay.clone()).drain().await.unwrap();
	assert_eq!(relay.calls.load(Ordering::SeqCst), 1);

	sqlx::query("DELETE FROM consilium_mail WHERE id = $1").bind(id).execute(&pool).await.unwrap();
}
