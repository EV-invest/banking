//! Integration tests for the payments plane — real Postgres, no mocks (project rule).
//! They run when `DATABASE_URL` is set and skip otherwise.
//!
//! Two halves. The first asserts what only the DATABASE states — the partial unique index
//! over fund-owned sources, its deliberate exemption for an investor's own claim, and the
//! composite foreign key that makes a consent seat belong to its payment's source user; none
//! of these has a Rust code path, which is the point of putting them in the schema. The
//! second drives [`PgPayments`] itself, so that every runtime `sqlx::query` in the adapter is
//! executed by a test — the check the compile-time macros would have given, bought back.
//!
//! `payments` carries a database-wide invariant ("one open order per fund-owned source"), so
//! every test here takes [`exclusive_payments`] and starts from [`reset_payments`]. The reset
//! runs at the START of each test so a panicking one cannot wedge the rest.

use std::sync::Arc;

use domain::{
	balance::{LedgerAccountKey, Party, TransferCode},
	consilium::ConsiliumId,
	error::DomainError,
	money::{Network, Usdt},
	payments::{PaymentDestination, PaymentEffect, PaymentId, PaymentOrder, PaymentReason, PaymentState, PaymentTerms},
	users::{Email, UserId},
	withdrawals::WithdrawalId,
};
use piggybank_core::{
	infrastructure::{custody::StubCustody, payments::PgPayments, relay::Relay, users::PgUsers},
	ports::{
		LedgerTransfer, UserRepository,
		payments::{ApprovalSeat, ConsentAudit, ConsentCredential, ConsentDecision, ExecutionOutcome, MAX_CODE_ATTEMPTS, PaymentFeed, PaymentFilter, PaymentRepository},
	},
};
use sqlx::PgPool;
use tokio::sync::Notify;
use uuid::Uuid;

mod common;

static PAYMENTS: std::sync::LazyLock<tokio::sync::Mutex<()>> = std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

async fn exclusive_payments() -> tokio::sync::MutexGuard<'static, ()> {
	PAYMENTS.lock().await
}

/// Clear everything this suite writes, including the outbox and event-log rows the orders
/// drained. The outbox is shared with the relay suites, so leaving `payment` rows behind
/// would hand a live relay money facts about claims this suite never funded.
async fn reset_payments(pool: &PgPool) {
	for statement in [
		"DELETE FROM payments",
		"DELETE FROM outbox WHERE aggregate = 'payment'",
		"DELETE FROM event_log WHERE aggregate = 'payment'",
	] {
		sqlx::query(statement).execute(pool).await.expect("clear the payments plane");
	}
}

/// A freshly provisioned investor. Each call mints its own auth subject, so tests never
/// contend for one user row.
async fn an_investor(pool: &PgPool) -> UserId {
	let users = PgUsers::new(pool.clone());
	let tag = Uuid::new_v4();
	let subject = domain::auth::AuthSubject::parse(&format!("payments-test-{tag}")).unwrap();
	let email = Email::parse(&format!("payments-{tag}@example.test")).unwrap();
	users.provision(subject, email, true).await.expect("provision an investor").id()
}

/// Insert one order directly. These tests are about what the SCHEMA refuses, so the rows go
/// in the way a buggy adapter would write them, with nothing in between to launder a violation.
async fn insert_payment(pool: &PgPool, id: Uuid, state: &str, from_kind: &str, from_id: Option<&str>, to_kind: &str, initiator: UserId) -> Result<(), sqlx::Error> {
	sqlx::query(
		"INSERT INTO payments (id, state, from_kind, from_id, to_kind, amount, reason, payload_hash, initiator_user_id, expires_at, decided_at) \
		 VALUES ($1, $2, $3, $4, $5, '1000', 'a test order', $6, $7, now() + interval '72 hours', CASE WHEN $2 = 'pending' THEN NULL ELSE now() END)",
	)
	.bind(id)
	.bind(state)
	.bind(from_kind)
	.bind(from_id)
	.bind(to_kind)
	.bind(vec![0u8; 32])
	.bind(initiator.raw())
	.execute(pool)
	.await
	.map(|_| ())
}

/// The whole of the concurrent-approval overdraw defence for the fund's own claims: a race
/// that cannot be created does not have to be won.
///
/// `NULLS NOT DISTINCT` on the index is what makes this hold for `piggybank` and `revenue`,
/// which carry no id — under the default rule their two NULL keys would be distinct and the
/// invariant would silently not exist.
#[tokio::test]
async fn only_one_order_may_be_open_against_a_fund_owned_claim() {
	let _guard = exclusive_payments().await;
	let Some(pool) = common::pool().await else {
		eprintln!("DATABASE_URL unset — skipping payments schema tests");
		return;
	};
	reset_payments(&pool).await;
	let initiator = an_investor(&pool).await;

	insert_payment(&pool, Uuid::new_v4(), "pending", "piggybank", None, "revenue", initiator)
		.await
		.expect("the first order against the fund's capital is accepted");

	let second = insert_payment(&pool, Uuid::new_v4(), "pending", "piggybank", None, "revenue", initiator).await;
	assert!(second.is_err(), "a second OPEN order against the same fund-owned claim must be refused");

	// `approved` still holds the source (the reservation is against it), so it counts as open
	// too — and a terminal order releases it.
	sqlx::query("UPDATE payments SET state = 'cancelled', decided_at = now()")
		.execute(&pool)
		.await
		.expect("close the first order");
	insert_payment(&pool, Uuid::new_v4(), "approved", "piggybank", None, "revenue", initiator)
		.await
		.expect("a closed order releases its source claim");
	let third = insert_payment(&pool, Uuid::new_v4(), "pending", "piggybank", None, "revenue", initiator).await;
	assert!(third.is_err(), "an approved order still holds its source claim");

	reset_payments(&pool).await;
}

/// THE EXEMPTION IS THE FEATURE. An investor's claim is already serialized by `lock_claim`
/// and backstopped by TigerBeetle's non-negative flag, so indexing it here would add no
/// safety while letting one pending consent block that investor's every other payment for up
/// to 72h — a denial of service dressed as a guarantee.
#[tokio::test]
async fn an_investor_may_have_several_orders_open_at_once() {
	let _guard = exclusive_payments().await;
	let Some(pool) = common::pool().await else {
		eprintln!("DATABASE_URL unset — skipping payments schema tests");
		return;
	};
	reset_payments(&pool).await;
	let investor = an_investor(&pool).await;
	let source = investor.to_string();

	for _ in 0..3 {
		insert_payment(&pool, Uuid::new_v4(), "pending", "user", Some(&source), "revenue", investor)
			.await
			.expect("an investor's own claim is deliberately outside the single-open index");
	}

	reset_payments(&pool).await;
}

/// §3's rule — "a payment whose source is `User(u)` requires `u`'s own consent" — made
/// unrepresentable otherwise. `payments.source_user_id` is GENERATED by the database from
/// `from_kind`/`from_id`, so the composite FK cannot be satisfied by a seat naming anyone
/// else, and a fund-owned order (whose generated column is NULL) can have no seat at all.
#[tokio::test]
async fn a_consent_seat_can_only_name_its_payments_own_source_user() {
	let _guard = exclusive_payments().await;
	let Some(pool) = common::pool().await else {
		eprintln!("DATABASE_URL unset — skipping payments schema tests");
		return;
	};
	reset_payments(&pool).await;
	let investor = an_investor(&pool).await;
	let stranger = an_investor(&pool).await;
	let own = Uuid::new_v4();
	let fund_owned = Uuid::new_v4();
	insert_payment(&pool, own, "pending", "user", Some(&investor.to_string()), "revenue", investor)
		.await
		.expect("open the investor's order");
	insert_payment(&pool, fund_owned, "pending", "revenue", None, "piggybank", investor)
		.await
		.expect("open a fund-owned order");

	assert!(seat(&pool, own, stranger).await.is_err(), "a seat naming another user must be refused");
	assert!(seat(&pool, fund_owned, investor).await.is_err(), "a fund-owned order has no consent subject to seat");
	seat(&pool, own, investor).await.expect("the source user's own seat is accepted");

	reset_payments(&pool).await;
}

async fn seat(pool: &PgPool, payment: Uuid, subject: UserId) -> Result<(), sqlx::Error> {
	sqlx::query(
		"INSERT INTO payment_consent (payment_id, subject_user_id, token_hash, code_hash, expires_at, subject_token_version_at_open, subject_email_hash_at_open) \
		 VALUES ($1, $2, $3, $4, now() + interval '72 hours', 0, $5)",
	)
	.bind(payment)
	.bind(subject.raw())
	.bind(Uuid::new_v4().as_bytes().repeat(2))
	.bind(vec![2u8; 32])
	.bind(vec![3u8; 32])
	.execute(pool)
	.await
	.map(|_| ())
}

// ---------------------------------------------------------------------------------------
// The adapter. Everything below drives `PgPayments`, so every runtime query it holds is
// executed at least once here — a runtime `sqlx::query` with no test that runs it is an
// unchecked string, which is the one thing the compile-time macros would have caught.
// ---------------------------------------------------------------------------------------

/// The digest the adapter compares a submitted code against.
fn digest(bytes: &[u8]) -> [u8; 32] {
	use sha2::{Digest, Sha256};
	Sha256::digest(bytes).into()
}

fn now() -> i64 {
	std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

fn usdt(decimal: &str) -> Usdt {
	Usdt::parse_decimal(decimal).unwrap()
}

fn audit() -> ConsentAudit {
	ConsentAudit {
		client_ip: "203.0.113.7".to_owned(),
		user_agent: "integration-test".to_owned(),
	}
}

/// An order and the seat credential whose plaintext code the test still holds.
fn an_order(from: Party, to: PaymentDestination, initiator: UserId, amount: &str) -> PaymentOrder {
	let terms = PaymentTerms::new(from, to, usdt(amount), PaymentReason::new("a documented reason").unwrap()).unwrap();
	// The application layer takes this hash; the tests only need one that is stable per order.
	let payload_hash = digest(&terms.canonical_bytes());
	PaymentOrder::open(PaymentId::from_raw(Uuid::new_v4()), terms, payload_hash, initiator, now())
}

const CODE: &str = "428913";

fn a_consent_seat(subject: UserId) -> ApprovalSeat {
	ApprovalSeat::Consent(ConsentCredential {
		subject,
		token_hash: digest(format!("token-{subject}-{}", Uuid::new_v4()).as_bytes()),
		code_hash: digest(CODE.as_bytes()),
		token_version_at_open: 0,
		email_hash_at_open: digest(b"subject@example.test"),
	})
}

fn token_hash_of(seat: &ApprovalSeat) -> [u8; 32] {
	match seat {
		ApprovalSeat::Consent(credential) => credential.token_hash,
		ApprovalSeat::Consilium(_) => unreachable!("this helper mints consent seats"),
	}
}

/// A consilium row in a TERMINAL state, standing in for the governance record a fund-owned
/// order links to. Terminal on purpose: `consilium_single_open_per_source_idx` covers only
/// the OPEN rows, so this fixture cannot block the governance suite that shares the table.
async fn a_decided_consilium(pool: &PgPool, initiator: UserId) -> ConsiliumId {
	let id = Uuid::new_v4();
	sqlx::query(
		"INSERT INTO consilium (id, kind, state, terms, source_claim, payload_hash, initiator_user_id, owner_count, threshold, expires_at, decided_at) \
		 VALUES ($1, 'revenue_payout', 'approved', $2::jsonb, 'fee', $3, $4, 3, 2, now() + interval '72 hours', now())",
	)
	.bind(id)
	.bind(r#"{"network":"bep20","address":"0x52908400098527886E0F7030069857D2E4169EE7","amount":"1","memo":"fixture"}"#)
	.bind(vec![9u8; 32])
	.bind(initiator.raw())
	.execute(pool)
	.await
	.expect("insert the consilium fixture");
	ConsiliumId::from_raw(id)
}

/// How many of this order's events reached the outbox. `Opened`, `Approved` and `Executed`
/// are audit trail and must not be there; only `Reserved` and `Settled` move money.
async fn relayed_kinds(pool: &PgPool, order: PaymentId) -> Vec<String> {
	sqlx::query_scalar("SELECT payload::jsonb ->> 'type' FROM outbox WHERE aggregate = 'payment' AND aggregate_id = $1 ORDER BY seq")
		.bind(order.raw())
		.fetch_all(pool)
		.await
		.expect("read the outbox")
}

/// Opening an order writes it, its seat and its events in one transaction — and puts NOTHING
/// in the outbox, because an order that has not been approved has moved no money.
#[tokio::test]
async fn opening_an_order_materializes_its_consent_seat_and_relays_nothing() {
	let _guard = exclusive_payments().await;
	let Some(pool) = common::pool().await else {
		eprintln!("DATABASE_URL unset — skipping payments adapter tests");
		return;
	};
	reset_payments(&pool).await;
	let investor = an_investor(&pool).await;
	let payments = PgPayments::new(pool.clone());

	let mut order = an_order(Party::User(investor), PaymentDestination::Internal(Party::Revenue), investor, "12.50");
	let id = order.id();
	payments.open(&mut order, a_consent_seat(investor)).await.expect("open the order");

	let view = payments.find(id).await.expect("find the order").expect("the order exists");
	assert_eq!(view.order.state(), PaymentState::Pending);
	assert_eq!(view.order.terms().amount(), usdt("12.50"));
	assert!(view.consilium_id.is_none(), "an investor-sourced order is not decided by a quorum");
	let consent = view.consent.expect("the seat was materialized with the order");
	assert_eq!(consent.subject, investor);
	assert_eq!(consent.decision, ConsentDecision::Pending);
	assert_eq!(consent.attempts_remaining, MAX_CODE_ATTEMPTS as u32);
	assert!(relayed_kinds(&pool, id).await.is_empty(), "a pending order has moved no money");

	reset_payments(&pool).await;
}

/// The five-attempt ceiling, and what happens at it. A burned consent FAILS THE PAYMENT
/// CLOSED — with one seat there is nobody to escalate to, so the exhausted token is a refusal
/// rather than a detector.
#[tokio::test]
async fn five_wrong_codes_burn_the_consent_and_close_the_order() {
	let _guard = exclusive_payments().await;
	let Some(pool) = common::pool().await else {
		eprintln!("DATABASE_URL unset — skipping payments adapter tests");
		return;
	};
	reset_payments(&pool).await;
	let investor = an_investor(&pool).await;
	let payments = PgPayments::new(pool.clone());
	let seat = a_consent_seat(investor);
	let token = token_hash_of(&seat);

	let mut order = an_order(Party::User(investor), PaymentDestination::Internal(Party::Revenue), investor, "1.00");
	let id = order.id();
	payments.open(&mut order, seat).await.expect("open the order");

	// Reading the invitation costs no attempt — mail scanners fetch every URL in a message.
	let invitation = payments.invitation(&token, now()).await.expect("the token resolves");
	assert_eq!(invitation.payment_id, id);
	assert_eq!(invitation.attempts_remaining, MAX_CODE_ATTEMPTS as u32);

	for remaining in (1..MAX_CODE_ATTEMPTS).rev() {
		let err = payments
			.submit(&token, "000000", ConsentDecision::Approve, &audit(), now())
			.await
			.expect_err("a wrong code is refused");
		assert!(
			matches!(err, DomainError::Validation(ref message) if message.contains(&format!("{remaining} attempts"))),
			"unexpected refusal: {err:?}"
		);
	}
	let burned = payments
		.submit(&token, "000000", ConsentDecision::Approve, &audit(), now())
		.await
		.expect_err("the fifth wrong code burns");
	assert!(matches!(burned, DomainError::NotFound { .. }), "a burned token answers exactly like an unknown one: {burned:?}");

	let view = payments.find(id).await.unwrap().unwrap();
	assert_eq!(view.order.state(), PaymentState::Rejected, "the burn fails the payment closed");
	assert!(relayed_kinds(&pool, id).await.is_empty(), "a refused order reserves nothing");
	// And the token is now indistinguishable from an unknown one on the read surface too.
	assert!(payments.invitation(&token, now()).await.is_err());

	reset_payments(&pool).await;
}

/// The happy path, and the two idempotency rules the retry contract rests on: the same answer
/// again is a no-op, a different one is a conflict.
#[tokio::test]
async fn the_right_code_approves_the_order_and_reserves_its_source() {
	let _guard = exclusive_payments().await;
	let Some(pool) = common::pool().await else {
		eprintln!("DATABASE_URL unset — skipping payments adapter tests");
		return;
	};
	reset_payments(&pool).await;
	let investor = an_investor(&pool).await;
	let payments = PgPayments::new(pool.clone());
	let seat = a_consent_seat(investor);
	let token = token_hash_of(&seat);

	let mut order = an_order(Party::User(investor), PaymentDestination::Internal(Party::Revenue), investor, "3.00");
	let id = order.id();
	payments.open(&mut order, seat).await.expect("open the order");

	let outcome = payments
		.submit(&token, CODE, ConsentDecision::Approve, &audit(), now())
		.await
		.expect("the right code is accepted");
	assert!(outcome.decided && outcome.approved);
	assert_eq!(outcome.payment.order.state(), PaymentState::Approved);
	assert_eq!(
		relayed_kinds(&pool, id).await,
		vec!["reserved".to_owned()],
		"approval reserves the source, and relays nothing else"
	);

	let repeat = payments
		.submit(&token, CODE, ConsentDecision::Approve, &audit(), now())
		.await
		.expect("a retried answer is a no-op");
	assert!(!repeat.decided, "a repeat must not double-count");
	let contradiction = payments.submit(&token, CODE, ConsentDecision::Reject, &audit(), now()).await;
	assert!(matches!(contradiction, Err(DomainError::Conflict(_))), "a different answer is refused, never an overwrite");

	// Executing it settles the reservation. `Settled` is the second and last event that moves
	// money; `Executed` is audit trail and must not reach the relay.
	payments
		.record_execution(id, ExecutionOutcome::Executed(PaymentEffect::Transfer), now())
		.await
		.expect("record the ledger effect");
	assert_eq!(relayed_kinds(&pool, id).await, vec!["reserved".to_owned(), "settled".to_owned()]);
	let view = payments.find(id).await.unwrap().unwrap();
	assert_eq!(view.order.state(), PaymentState::Executed);
	assert!(payments.awaiting_execution().await.unwrap().is_empty(), "an executed order is not awaiting execution");

	reset_payments(&pool).await;
}

/// The fund-owned branch: the order links the consilium that decides it, the approval is
/// recorded by the quorum rather than by a token, and execution is idempotent for the same
/// effect and a conflict for a different one.
#[tokio::test]
async fn a_fund_owned_order_is_carried_by_its_consilium() {
	let _guard = exclusive_payments().await;
	let Some(pool) = common::pool().await else {
		eprintln!("DATABASE_URL unset — skipping payments adapter tests");
		return;
	};
	reset_payments(&pool).await;
	let operator = an_investor(&pool).await;
	let payments = PgPayments::new(pool.clone());
	let consilium = a_decided_consilium(&pool, operator).await;

	let mut order = an_order(Party::Revenue, PaymentDestination::Internal(Party::Piggybank), operator, "40.00");
	let id = order.id();
	payments.open(&mut order, ApprovalSeat::Consilium(consilium)).await.expect("open the order");

	let view = payments.find(id).await.unwrap().unwrap();
	assert_eq!(view.consilium_id, Some(consilium));
	assert!(view.consent.is_none(), "fund-owned money is never consented to by one investor");

	assert_eq!(payments.awaiting_execution().await.unwrap(), Vec::new(), "a pending order is not executable");
	payments.record_approval(id, now()).await.expect("the quorum carried it");
	assert_eq!(payments.awaiting_execution().await.unwrap(), vec![id]);
	assert_eq!(relayed_kinds(&pool, id).await, vec!["reserved".to_owned()]);
	payments.record_approval(id, now()).await.expect("recording the same approval twice is a no-op");

	payments.record_execution(id, ExecutionOutcome::Executed(PaymentEffect::Transfer), now()).await.expect("execute");
	let conflict = payments
		.record_execution(id, ExecutionOutcome::Executed(PaymentEffect::Withdrawal(WithdrawalId::from_raw(Uuid::new_v4()))), now())
		.await;
	assert!(matches!(conflict, Err(DomainError::Conflict(_))), "a second, different effect is a conflict, not an overwrite");

	sqlx::query("DELETE FROM consilium WHERE id = $1").bind(consilium.raw()).execute(&pool).await.ok();
	reset_payments(&pool).await;
}

/// The admin feed's filters, the expiry sweep, and the rule that only the operator who opened
/// an order may withdraw it.
#[tokio::test]
async fn the_feed_filters_and_the_sweep_close_what_nobody_answered() {
	let _guard = exclusive_payments().await;
	let Some(pool) = common::pool().await else {
		eprintln!("DATABASE_URL unset — skipping payments adapter tests");
		return;
	};
	reset_payments(&pool).await;
	let investor = an_investor(&pool).await;
	let stranger = an_investor(&pool).await;
	let payments = PgPayments::new(pool.clone());

	let mut of_investor = an_order(Party::User(investor), PaymentDestination::Internal(Party::Revenue), investor, "5.00");
	let investors_order = of_investor.id();
	payments.open(&mut of_investor, a_consent_seat(investor)).await.expect("open the investor's order");
	let mut of_fund = an_order(Party::Revenue, PaymentDestination::Internal(Party::Piggybank), investor, "9.00");
	let fund_order = of_fund.id();
	payments
		.open(&mut of_fund, ApprovalSeat::Consilium(a_decided_consilium(&pool, investor).await))
		.await
		.expect("open the fund's order");

	let all = payments.list(&PaymentFilter::default(), 50).await.expect("the whole history");
	assert_eq!(all.len(), 2);
	let fund_owned = payments
		.list(
			&PaymentFilter {
				fund_owned_source: Some(true),
				..PaymentFilter::default()
			},
			50,
		)
		.await
		.unwrap();
	assert_eq!(fund_owned.iter().map(|view| view.order.id()).collect::<Vec<_>>(), vec![fund_order]);
	let by_party = payments
		.list(
			&PaymentFilter {
				party: Some(Party::User(investor)),
				state: Some(PaymentState::Pending),
				..PaymentFilter::default()
			},
			50,
		)
		.await
		.unwrap();
	assert_eq!(by_party.iter().map(|view| view.order.id()).collect::<Vec<_>>(), vec![investors_order]);

	let refused = payments.cancel(fund_order, stranger, now()).await;
	assert!(matches!(refused, Err(DomainError::Forbidden(_))), "only the operator who opened it may withdraw it");
	payments.cancel(fund_order, investor, now()).await.expect("the initiator may withdraw it");

	// The sweep binds the caller's clock, so "past the deadline" and "expired" answer the same
	// question. One tick past the TTL closes exactly the order still pending.
	let closed = payments.expire_due(now() + domain::payments::TTL_SECS + 1).await.expect("sweep");
	assert_eq!(closed, 1, "the cancelled order is already closed; only the pending one expires");
	assert_eq!(payments.find(investors_order).await.unwrap().unwrap().order.state(), PaymentState::Expired);

	sqlx::query("DELETE FROM consilium WHERE initiator_user_id = $1").bind(investor.raw()).execute(&pool).await.ok();
	reset_payments(&pool).await;
}

/// `Approved` IS A COMMITTED STATE: the reservation landed in the approval's transaction and
/// the settlement would land in the execution's. A failure between the two must give the
/// locked amount back, or a terminal order leaves it in `clearing` with nothing left to post
/// or void it — a withdrawal's fail void, by another name. `fund` is a global singleton
/// shared with every other suite, so every figure here is a DELTA.
#[tokio::test]
async fn a_failed_execution_releases_the_reservation_it_was_holding() {
	let _guard = exclusive_payments().await;
	let Some(pool) = common::pool().await else {
		eprintln!("DATABASE_URL unset — skipping payments adapter tests");
		return;
	};
	let Some(ledger) = common::seeded_ledger(&pool, "payments release test").await else {
		return;
	};
	reset_payments(&pool).await;
	let investor = an_investor(&pool).await;
	let payments = PgPayments::new(pool.clone());
	let notify = Arc::new(Notify::new());
	let relay = Relay::new(pool.clone(), ledger.clone(), Arc::new(StubCustody), notify.clone());
	ledger
		.post(&LedgerTransfer {
			id: Uuid::new_v4().as_u128(),
			debit: LedgerAccountKey::CryptoWallet(Network::Bep20),
			credit: LedgerAccountKey::Fund,
			amount: usdt("100").base_units(),
			code: TransferCode::Deposit,
			reference: 0,
		})
		.await
		.expect("fund the fund's own claim");
	let before = ledger.balance(&LedgerAccountKey::Fund).await.unwrap();
	let revenue_before = ledger.balance(&LedgerAccountKey::FeeRevenue).await.unwrap().posted;

	let mut order = an_order(Party::Piggybank, PaymentDestination::Internal(Party::Revenue), investor, "30");
	let id = order.id();
	payments
		.open(&mut order, ApprovalSeat::Consilium(a_decided_consilium(&pool, investor).await))
		.await
		.expect("open the order");
	payments.record_approval(id, now()).await.expect("the quorum carried it");
	relay.drain().await;
	let reserved = ledger.balance(&LedgerAccountKey::Fund).await.unwrap();
	assert_eq!(reserved.locked - before.locked, usdt("30").base_units(), "the approval locked the source");

	payments
		.record_execution(id, ExecutionOutcome::Failed("the ledger refused the settlement".into()), now())
		.await
		.expect("record the failure");
	assert_eq!(
		relayed_kinds(&pool, id).await,
		vec!["reserved".to_owned(), "released".to_owned()],
		"the failure relays exactly one money fact: the release"
	);
	relay.drain().await;

	let released = ledger.balance(&LedgerAccountKey::Fund).await.unwrap();
	assert_eq!(released.posted, before.posted, "nothing was debited");
	assert_eq!(released.locked, before.locked, "the reservation was voided");
	assert_eq!(ledger.balance(&LedgerAccountKey::FeeRevenue).await.unwrap().posted, revenue_before, "the destination saw nothing");
	let parked: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM outbox WHERE aggregate = 'payment' AND parked_at IS NOT NULL")
		.fetch_one(&pool)
		.await
		.unwrap();
	assert_eq!(parked, 0, "a parked row would mean a void the ledger refused");
	let view = payments.find(id).await.unwrap().unwrap();
	assert_eq!(view.order.state(), PaymentState::ExecutionFailed);
	assert_eq!(view.order.failure_reason(), Some("the ledger refused the settlement"));
	assert!(payments.awaiting_execution().await.unwrap().is_empty(), "nothing retries a failure silently");

	sqlx::query("DELETE FROM consilium WHERE initiator_user_id = $1").bind(investor.raw()).execute(&pool).await.ok();
	reset_payments(&pool).await;
}

/// THE ONE TEST THAT PROVES THE MONEY MOVES. Everything above asserts Postgres rows; this
/// drives the whole write path — order, consent, relay, TigerBeetle — and reads the claims
/// back from the ledger that actually owns them.
///
/// It also pins the two-phase shape: after the approval the source is *locked*, not yet
/// debited, so a second approved order against the same claim contends with a reservation
/// rather than with a stale read. Only the settlement posts it.
#[tokio::test]
async fn an_approved_payment_reserves_its_source_and_then_settles_it() {
	let _guard = exclusive_payments().await;
	let Some(pool) = common::pool().await else {
		eprintln!("DATABASE_URL unset — skipping payments adapter tests");
		return;
	};
	let Some(ledger) = common::seeded_ledger(&pool, "payments relay test").await else {
		return;
	};
	reset_payments(&pool).await;
	let investor = an_investor(&pool).await;
	let payments = PgPayments::new(pool.clone());
	let notify = Arc::new(Notify::new());
	let relay = Relay::new(pool.clone(), ledger.clone(), Arc::new(StubCustody), notify.clone());

	// Credit the investor's claim the way a deposit does: `Dr wallet:<net> / Cr user:<id>`.
	ledger
		.post(&LedgerTransfer {
			id: Uuid::new_v4().as_u128(),
			debit: LedgerAccountKey::CryptoWallet(Network::Bep20),
			credit: LedgerAccountKey::UserClaim(investor),
			amount: usdt("100").base_units(),
			code: TransferCode::Deposit,
			reference: 0,
		})
		.await
		.expect("fund the investor's claim");

	let seat = a_consent_seat(investor);
	let token = token_hash_of(&seat);
	let mut order = an_order(Party::User(investor), PaymentDestination::Internal(Party::Revenue), investor, "30");
	let id = order.id();
	payments.open(&mut order, seat).await.expect("open the order");
	payments.submit(&token, CODE, ConsentDecision::Approve, &audit(), now()).await.expect("consent");

	relay.drain().await;
	let reserved = ledger.balance(&LedgerAccountKey::UserClaim(investor)).await.unwrap();
	assert_eq!(reserved.posted, usdt("100").base_units(), "a reservation locks the claim, it does not debit it");
	assert_eq!(reserved.locked, usdt("30").base_units());
	assert_eq!(reserved.available(), usdt("70").base_units());

	let revenue_before = ledger.balance(&LedgerAccountKey::FeeRevenue).await.unwrap().posted;
	payments.record_execution(id, ExecutionOutcome::Executed(PaymentEffect::Transfer), now()).await.expect("execute");
	relay.drain().await;

	let settled = ledger.balance(&LedgerAccountKey::UserClaim(investor)).await.unwrap();
	assert_eq!(settled.posted, usdt("70").base_units(), "the settlement posts the reservation");
	assert_eq!(settled.locked, 0);
	// `fee` is a global singleton shared with every other suite, so this is a DELTA.
	let revenue_after = ledger.balance(&LedgerAccountKey::FeeRevenue).await.unwrap().posted;
	assert_eq!(revenue_after - revenue_before, usdt("30").base_units());
	// Nothing parked: a parked row here would mean a leg the ledger refused.
	let parked: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM outbox WHERE aggregate = 'payment' AND parked_at IS NOT NULL")
		.fetch_one(&pool)
		.await
		.unwrap();
	assert_eq!(parked, 0);

	reset_payments(&pool).await;
}
