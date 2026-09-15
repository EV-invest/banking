//! The mail worker against a real queue, with the relay stubbed at its port — real Postgres,
//! no DB mocks (project rule); the identity plane's relay is the one external gateway here,
//! stood in for the way `StubCustody` stands in for the signer.
//!
//! What is pinned: a relay that THROTTLES a recipient (`RESOURCE_EXHAUSTED`) or is down
//! defers the mail without spending one of its attempts, the deferred row waits out its
//! backoff before the relay is asked again, an actual refusal still costs an attempt, a
//! mail deferred for longer than the ceiling is finally given up on, and an outcome over a
//! change of fee terms reaches the relay with the fund and the terms it was queued with —
//! while a row queued before those fields existed still gets through.

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
		governance_mail::{FeePolicyTerms, GovernanceMail, GovernanceMailer, MailDeliveryError, PayoutApproval, PayoutOutcome},
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

/// A relay that accepts everything and keeps what it was handed, so a test can read the
/// mail exactly as the worker rebuilt it from the queue row.
#[derive(Default)]
struct KeepingRelay {
	seen: std::sync::Mutex<Vec<GovernanceMail>>,
}

#[async_trait]
impl GovernanceMailer for KeepingRelay {
	async fn send(&self, _recipient: Uuid, _dedupe_key: &str, mail: &GovernanceMail) -> Result<(), MailDeliveryError> {
		self.seen.lock().unwrap().push(mail.clone());
		Ok(())
	}
}

/// A recipient with a mirrored concierge id, a terminal consilium fixture to hang the mail
/// on, and one queued outcome mail. Returns the mail row's id.
async fn a_queued_mail(pool: &PgPool) -> i64 {
	a_queued(pool, |consilium| {
		GovernanceMail::PayoutOutcome(PayoutOutcome {
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
			fund: String::new(),
			current: None,
			proposed: None,
		})
	})
	.await
}

/// The same fixture, carrying a token and a code — the kind whose payload holds a secret.
async fn a_queued_token_mail(pool: &PgPool) -> i64 {
	a_queued(pool, |consilium| {
		GovernanceMail::PayoutApproval(PayoutApproval {
			consilium_id: consilium.to_string(),
			initiator_email: "owner@example.test".into(),
			network: "bep20".into(),
			address: "0x52908400098527886E0F7030069857D2E4169EE7".into(),
			amount: "1".into(),
			memo: "fixture".into(),
			payload_hash: "00".repeat(32),
			threshold: 2,
			owner_count: 3,
			expires_at: 0,
			approval_url: "https://example.test/approve/SECRET-TOKEN".into(),
			code: "SECRET-CODE".into(),
		})
	})
	.await
}

async fn a_queued(pool: &PgPool, mail: impl FnOnce(Uuid) -> GovernanceMail) -> i64 {
	a_queued_row(pool, |consilium| {
		let mail = mail(consilium);
		(mail.as_str(), serde_json::to_string(&mail).unwrap())
	})
	.await
}

/// The same fixture over the row's stored `kind` and JSON `payload` — for a payload written
/// the way an EARLIER build of the worker wrote it, which no current type can produce.
async fn a_queued_row(pool: &PgPool, row: impl FnOnce(Uuid) -> (&'static str, String)) -> i64 {
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
	let (kind, payload) = row(consilium);
	sqlx::query_scalar("INSERT INTO consilium_mail (consilium_id, user_id, kind, dedupe_key, payload) VALUES ($1, $2, $3, $4, $5::jsonb) RETURNING id")
		.bind(consilium)
		.bind(owner.raw())
		.bind(kind)
		.bind(format!("mailer-test:{tag}"))
		.bind(payload)
		.fetch_one(pool)
		.await
		.unwrap()
}

fn terms(management_bps: u32) -> FeePolicyTerms {
	FeePolicyTerms {
		management_bps,
		performance_bps: 2_000,
		hurdle_bps: 0,
		basis: "invested_capital".into(),
		crystallization: "annual".into(),
	}
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

/// A row the mailer gives up on — ten refusals, or a day of deferrals — keeps its audit
/// trail but not its secrets: the token and the code are stripped exactly as they are on
/// delivery, because a credential nobody will ever receive is one nobody should hold.
#[tokio::test]
async fn a_mail_given_up_on_is_redacted_like_a_delivered_one() {
	let _guard = QUEUE.lock().await;
	let Some(pool) = common::pool().await else {
		eprintln!("DATABASE_URL unset — skipping the mailer suite");
		return;
	};
	quiet_queue(&pool).await;

	let refused = a_queued_token_mail(&pool).await;
	let relay = refusing();
	for _ in 0..10 {
		ConsiliumMailer::new(pool.clone(), relay.clone()).drain().await.unwrap();
	}
	let deferred = a_queued_token_mail(&pool).await;
	sqlx::query("UPDATE consilium_mail SET created_at = now() - interval '25 hours' WHERE id = $1")
		.bind(deferred)
		.execute(&pool)
		.await
		.unwrap();
	ConsiliumMailer::new(pool.clone(), throttling()).drain().await.unwrap();

	for id in [refused, deferred] {
		let (attempts, _, _, sent) = row(&pool, id).await;
		assert!(attempts >= 10 && !sent, "retired: attempts={attempts}");
		let payload: String = sqlx::query_scalar("SELECT payload::text FROM consilium_mail WHERE id = $1")
			.bind(id)
			.fetch_one(&pool)
			.await
			.unwrap();
		assert!(!payload.contains("SECRET"), "the retired row holds no secret: {payload}");
		let mail: serde_json::Value = serde_json::from_str(&payload).unwrap();
		assert_eq!(mail["kind"], "payout_approval", "the audit trail keeps what was attempted");
		assert_eq!(mail["memo"], "fixture");
	}

	sqlx::query("DELETE FROM consilium_mail WHERE id = ANY($1)")
		.bind(vec![refused, deferred])
		.execute(&pool)
		.await
		.unwrap();
}

/// A burn notice over a change of fee terms carries the fund line and both sets of terms
/// through the queue and out to the relay — the description concierge v0.8.0 renders — with
/// `current` kept absent when the fund charged nothing and no reason, as the contract has it
/// under the burn kind; the verdict beside it keeps the initiator's reason.
#[tokio::test]
async fn a_fee_terms_outcome_reaches_the_relay_with_its_fund_and_terms() {
	let _guard = QUEUE.lock().await;
	let Some(pool) = common::pool().await else {
		eprintln!("DATABASE_URL unset — skipping the mailer suite");
		return;
	};
	quiet_queue(&pool).await;
	let burned = a_queued(&pool, |consilium| {
		GovernanceMail::TokenBurned(PayoutOutcome {
			consilium_id: consilium.to_string(),
			outcome: "TOKEN_BURNED".into(),
			network: String::new(),
			address: String::new(),
			amount: String::new(),
			detail: "five failed code attempts burned the approval token for seat fixture".into(),
			tier: String::new(),
			source: String::new(),
			destination: String::new(),
			reason: String::new(),
			fund: "Arb desk (service_arb)".into(),
			current: None,
			proposed: Some(terms(300)),
		})
	})
	.await;
	let rejected = a_queued(&pool, |consilium| {
		GovernanceMail::PayoutOutcome(PayoutOutcome {
			consilium_id: consilium.to_string(),
			outcome: "REJECTED".into(),
			network: String::new(),
			address: String::new(),
			amount: String::new(),
			detail: "the threshold can no longer be reached".into(),
			tier: String::new(),
			source: String::new(),
			destination: String::new(),
			reason: "the new mandate costs more to run".into(),
			fund: "Arb desk (service_arb)".into(),
			current: Some(terms(200)),
			proposed: Some(terms(300)),
		})
	})
	.await;
	let relay = Arc::new(KeepingRelay::default());
	assert_eq!(ConsiliumMailer::new(pool.clone(), relay.clone()).drain().await.unwrap(), 2);

	let seen = relay.seen.lock().unwrap().clone();
	let GovernanceMail::TokenBurned(burn) = &seen[0] else {
		panic!("the burn notice keeps its kind through the queue: {:?}", seen[0]);
	};
	assert_eq!(burn.outcome, "TOKEN_BURNED");
	assert_eq!(burn.fund, "Arb desk (service_arb)");
	assert!(burn.current.is_none(), "a fund that charged nothing has no current terms");
	assert_eq!(burn.proposed.as_ref().map(|t| t.management_bps), Some(300));
	assert!(burn.reason.is_empty(), "a burn notice carries no initiator's note");
	assert!(burn.network.is_empty() && burn.source.is_empty() && burn.destination.is_empty(), "one description, not two");
	let GovernanceMail::PayoutOutcome(outcome) = &seen[1] else {
		panic!("the outcome keeps its kind through the queue: {:?}", seen[1]);
	};
	assert_eq!(outcome.outcome, "REJECTED");
	assert_eq!(outcome.reason, "the new mandate costs more to run");
	assert_eq!(outcome.current.as_ref().map(|t| t.management_bps), Some(200));
	assert_eq!(outcome.proposed.as_ref().map(|t| t.crystallization.as_str()), Some("annual"));

	for id in [burned, rejected] {
		let (_, _, _, sent) = row(&pool, id).await;
		assert!(sent, "an accepted mail is marked sent");
	}
	sqlx::query("DELETE FROM consilium_mail WHERE id = ANY($1)")
		.bind(vec![burned, rejected])
		.execute(&pool)
		.await
		.unwrap();
}

/// A row queued by the worker as it was before the fee description existed — the payout
/// pair alone, no `fund`, `current` or `proposed` key at all — is still a mail the current
/// worker can rebuild and hand over: the queue outlives a deploy.
#[tokio::test]
async fn an_outcome_row_queued_before_the_fee_fields_existed_still_reaches_the_relay() {
	let _guard = QUEUE.lock().await;
	let Some(pool) = common::pool().await else {
		eprintln!("DATABASE_URL unset — skipping the mailer suite");
		return;
	};
	quiet_queue(&pool).await;
	let id = a_queued_row(&pool, |consilium| {
		(
			"payout_outcome",
			format!(
				r#"{{"kind":"payout_outcome","consilium_id":"{consilium}","outcome":"CANCELLED","network":"bep20","address":"0x52908400098527886E0F7030069857D2E4169EE7","amount":"1","detail":"fixture"}}"#
			),
		)
	})
	.await;
	let relay = Arc::new(KeepingRelay::default());
	assert_eq!(ConsiliumMailer::new(pool.clone(), relay.clone()).drain().await.unwrap(), 1);

	let seen = relay.seen.lock().unwrap().clone();
	let GovernanceMail::PayoutOutcome(outcome) = &seen[0] else {
		panic!("an old row is still an outcome: {:?}", seen[0]);
	};
	assert_eq!((outcome.outcome.as_str(), outcome.network.as_str()), ("CANCELLED", "bep20"));
	assert!(
		outcome.fund.is_empty() && outcome.current.is_none() && outcome.proposed.is_none(),
		"absent keys read as the empty description"
	);
	assert!(outcome.tier.is_empty() && outcome.reason.is_empty());

	let (_, _, _, sent) = row(&pool, id).await;
	assert!(sent);
	sqlx::query("DELETE FROM consilium_mail WHERE id = $1").bind(id).execute(&pool).await.unwrap();
}
