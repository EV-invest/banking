//! Integration tests for the payments plane — real Postgres, no mocks (project rule).
//! They run when `DATABASE_URL` is set and skip otherwise.
//!
//! What is asserted HERE rather than in a unit test is exactly what only the database
//! states: the partial unique index over fund-owned sources, its deliberate exemption for an
//! investor's own claim, and the composite foreign key that makes a consent seat belong to
//! its payment's source user. None of these has a Rust code path to test — that is the point
//! of putting them in the schema.
//!
//! `payments` carries a database-wide invariant ("one open order per fund-owned source"), so
//! every test here takes [`exclusive_payments`] and starts from [`reset_payments`]. The reset
//! runs at the START of each test so a panicking one cannot wedge the rest.

use domain::users::{Email, UserId};
use piggybank_core::{infrastructure::users::PgUsers, ports::UserRepository};
use sqlx::PgPool;
use uuid::Uuid;

mod common;

static PAYMENTS: std::sync::LazyLock<tokio::sync::Mutex<()>> = std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

async fn exclusive_payments() -> tokio::sync::MutexGuard<'static, ()> {
	PAYMENTS.lock().await
}

async fn reset_payments(pool: &PgPool) {
	sqlx::query("DELETE FROM payments").execute(pool).await.expect("clear payments");
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

/// Insert one order directly. The adapter does not exist yet, and these tests are about what
/// the SCHEMA refuses — so the rows are written the way a future adapter (or a buggy one)
/// would write them, with nothing in between to launder a violation.
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
