//! Integration tests for the consilium — real Postgres **and** TigerBeetle (no mocks, per
//! the project rules). They run when `DATABASE_URL` is set and a TigerBeetle replica is
//! reachable (`nix run .#db` + `.#tb`), and skip otherwise.
//!
//! Two pieces of shared state force these tests to run one at a time, and both are
//! deliberate features of the design rather than test friction:
//!   - **at most one consilium may be OPEN across the whole database** (the partial unique
//!     index that removes the concurrent-approval race), and
//!   - **the owner roster is global** — `users.role = 'owner'` has no per-test scope, since
//!     the fund has exactly one set of owners.
//!
//! So every test takes [`exclusive_governance`] and starts from [`reset_governance`], which
//! closes any lingering open consilium and clears the roster. That setup is self-healing: a
//! test that panics half-way leaves state behind, and the next test's reset removes it.

use std::sync::Arc;

use domain::{
	auth::AuthSubject,
	balance::{LedgerAccountKey, TransferCode},
	consilium::{ConsiliumId, ConsiliumState, RevenuePayoutTerms, VoteDecision},
	error::DomainError,
	money::{Network, Usdt, WalletAddress},
	users::{Email, UserId},
};
use piggybank_core::{
	application::consilium as consilium_app,
	infrastructure::{consilium::PgConsilia, custody::StubCustody, relay::Relay, users::PgUsers, withdrawals::PgWithdrawals},
	ports::{
		ConsiliumRepository, LedgerTransfer, UserRepository, WithdrawalRepository,
		consilium::{ConsiliumView, MAX_CODE_ATTEMPTS, VoteAudit},
		ledger::Ledger,
	},
};
use sqlx::{PgPool, Row};
use tokio::sync::Notify;
use uuid::Uuid;

mod common;

/// A valid BEP20 destination — the payout's on-chain target in every test here.
const PAYOUT_ADDRESS: &str = "0x52908400098527886E0F7030069857D2E4169EE7";

/// The one rail these tests configure. `check_revenue_payout` refuses an unconfigured one,
/// which is a behaviour of its own (asserted in `an_impossible_payout_is_refused_at_open`).
const CONFIGURED: [Network; 1] = [Network::Bep20];

const APPROVAL_URL_BASE: &str = "https://example.test/consilium";

/// Serialises every test in this file — see the module docs.
static GOVERNANCE: std::sync::LazyLock<tokio::sync::Mutex<()>> = std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

async fn exclusive_governance() -> tokio::sync::MutexGuard<'static, ()> {
	GOVERNANCE.lock().await
}

struct Harness {
	pool: PgPool,
	consilia: Arc<dyn ConsiliumRepository>,
	withdrawals: Arc<dyn WithdrawalRepository>,
	users: Arc<dyn UserRepository>,
	ledger: Arc<dyn Ledger>,
	relay: Relay,
	notify: Arc<Notify>,
}

async fn harness() -> Option<Harness> {
	let pool = common::pool().await?;
	let ledger = common::seeded_ledger(&pool, "consilium test").await?;
	let notify = Arc::new(Notify::new());
	Some(Harness {
		consilia: Arc::new(PgConsilia::new(pool.clone())),
		withdrawals: Arc::new(PgWithdrawals::new(pool.clone())),
		users: Arc::new(PgUsers::new(pool.clone())),
		relay: Relay::new(pool.clone(), ledger.clone(), Arc::new(StubCustody), notify.clone()),
		ledger,
		notify,
		pool,
	})
}

fn ports(h: &Harness) -> consilium_app::ConsiliumPorts<'_> {
	consilium_app::ConsiliumPorts {
		consilia: h.consilia.as_ref(),
		withdrawals: h.withdrawals.as_ref(),
		ledger: h.ledger.as_ref(),
		custody: &StubCustody,
		relay: &h.notify,
		configured: &CONFIGURED,
		approval_url_base: APPROVAL_URL_BASE,
		// The suite exercises the governance path itself, so it stands in for a wired mailer.
		// `opening_without_a_governance_mailer_is_refused` pins the false case explicitly.
		governance_mail_wired: true,
	}
}

fn usdt(decimal: &str) -> Usdt {
	Usdt::parse_decimal(decimal).unwrap()
}

fn now() -> i64 {
	std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

fn terms(amount: &str) -> RevenuePayoutTerms {
	RevenuePayoutTerms::new(
		Network::Bep20,
		WalletAddress::parse(Network::Bep20, PAYOUT_ADDRESS).unwrap(),
		usdt(amount),
		"quarterly draw".to_owned(),
	)
	.unwrap()
}

/// Clear the governance state this suite shares and guarantee the fund has revenue to
/// propose paying out: close any open consilium (the unique index allows only one), empty
/// the owner roster, and top the `fee` claim up.
///
/// Run at the START of every test rather than the end, so a panicking test cannot wedge the
/// ones after it. The top-up is what makes the tests order-independent: opening a consilium
/// runs the real solvency pre-check against the `fee` claim, so a sibling test that spends
/// the fund's revenue would otherwise decide whether this one can open at all.
async fn reset_governance(h: &Harness) {
	sqlx::query("UPDATE consilium SET state = 'cancelled', decided_at = now() WHERE state = 'open'")
		.execute(&h.pool)
		.await
		.unwrap();
	sqlx::query("UPDATE users SET role = 'investor' WHERE role = 'owner'").execute(&h.pool).await.unwrap();
	// The cooling-off clock is global, so a test that exercises it would otherwise freeze
	// every test after it for 48 simulated hours.
	sqlx::query("DELETE FROM governance_roster_change").execute(&h.pool).await.unwrap();
	fund_revenue(h, "100000").await;
}

/// Provision a fresh user and seat them as a fund owner. `concierge_user_id` is set because
/// the mail worker addresses recipients by their id in the plane that owns identities.
async fn owner(h: &Harness) -> UserId {
	let subject = AuthSubject::parse(&format!("itest-{}", Uuid::new_v4())).unwrap();
	let email = Email::parse(&format!("o{}@example.com", Uuid::new_v4().simple())).unwrap();
	let id = h.users.provision(subject, email, true).await.unwrap().id();
	sqlx::query("UPDATE users SET role = 'owner', concierge_user_id = $2 WHERE id = $1")
		.bind(id.raw())
		.bind(Uuid::new_v4())
		.execute(&h.pool)
		.await
		.unwrap();
	id
}

/// Seat `n` owners. The first is the initiator in every test that opens a consilium.
async fn owners(h: &Harness, n: usize) -> Vec<UserId> {
	let mut roster = Vec::with_capacity(n);
	for _ in 0..n {
		roster.push(owner(h).await);
	}
	roster
}

/// Strip a user's seat — what the bridge does when concierge reports a role change.
async fn demote(h: &Harness, user: UserId) {
	sqlx::query("UPDATE users SET role = 'investor' WHERE id = $1").bind(user.raw()).execute(&h.pool).await.unwrap();
}

/// Credit the fund's earned revenue directly, the deposit shape (`Dr wallet / Cr fee`).
/// `fee` is a global singleton, so every assertion about it here is a DELTA.
async fn fund_revenue(h: &Harness, amount: &str) {
	h.ledger
		.post(&LedgerTransfer {
			id: Uuid::new_v4().as_u128(),
			debit: LedgerAccountKey::CryptoWallet(Network::Bep20),
			credit: LedgerAccountKey::FeeRevenue,
			amount: usdt(amount).base_units(),
			code: TransferCode::WithdrawFee,
			reference: 0,
		})
		.await
		.unwrap();
}

/// Move earned revenue back out of the `fee` claim — how a competing spend leaves a payout
/// with nothing behind it by the time it executes.
async fn drain_revenue(h: &Harness, base_units: u128) {
	h.ledger
		.post(&LedgerTransfer {
			id: Uuid::new_v4().as_u128(),
			debit: LedgerAccountKey::FeeRevenue,
			credit: LedgerAccountKey::CryptoWallet(Network::Bep20),
			amount: base_units,
			code: TransferCode::WithdrawFee,
			reference: 0,
		})
		.await
		.unwrap();
}

/// The token and code that were mailed to one seat. Reading them out of the queue is exactly
/// what the owner does when the message lands — the plaintexts live nowhere else.
async fn credentials(h: &Harness, consilium: ConsiliumId, voter: UserId) -> (String, String) {
	let payload: String = sqlx::query_scalar("SELECT payload::text FROM consilium_mail WHERE consilium_id = $1 AND user_id = $2 AND kind = 'payout_approval'")
		.bind(consilium.raw())
		.bind(voter.raw())
		.fetch_one(&h.pool)
		.await
		.expect("every eligible seat is mailed an approval");
	let mail: serde_json::Value = serde_json::from_str(&payload).unwrap();
	let url = mail["approval_url"].as_str().unwrap().to_owned();
	let token = url.rsplit('/').next().unwrap().to_owned();
	(token, mail["code"].as_str().unwrap().to_owned())
}

/// Cast one seat's vote with its real emailed credentials.
async fn vote(h: &Harness, consilium: ConsiliumId, voter: UserId, decision: VoteDecision) -> Result<bool, DomainError> {
	let (token, code) = credentials(h, consilium, voter).await;
	let audit = VoteAudit {
		client_ip: "203.0.113.7".to_owned(),
		user_agent: "itest".to_owned(),
	};
	consilium_app::submit_decision(h.consilia.as_ref(), &token, &code, decision, &audit, now())
		.await
		.map(|outcome| outcome.decided)
}

async fn state_of(h: &Harness, id: ConsiliumId) -> ConsiliumState {
	consilium_app::find(h.consilia.as_ref(), id).await.unwrap().consilium.state()
}

async fn open_payout(h: &Harness, initiator: UserId, amount: &str) -> ConsiliumView {
	consilium_app::open_revenue_payout(&ports(h), initiator, terms(amount), now()).await.unwrap()
}

/// Push a consilium and its tokens past their deadline, standing in for 72h passing.
async fn expire_the_window(h: &Harness, id: ConsiliumId) {
	sqlx::query("UPDATE consilium SET expires_at = now() - interval '1 hour' WHERE id = $1")
		.bind(id.raw())
		.execute(&h.pool)
		.await
		.unwrap();
	sqlx::query("UPDATE consilium_voter SET expires_at = now() - interval '1 hour' WHERE consilium_id = $1")
		.bind(id.raw())
		.execute(&h.pool)
		.await
		.unwrap();
}

/// One seat's attempt counter, straight from the column the CHECK constrains.
async fn attempts_of(h: &Harness, consilium: ConsiliumId, voter: UserId) -> i32 {
	sqlx::query_scalar("SELECT attempts FROM consilium_voter WHERE consilium_id = $1 AND user_id = $2")
		.bind(consilium.raw())
		.bind(voter.raw())
		.fetch_one(&h.pool)
		.await
		.unwrap()
}

async fn burned_of(h: &Harness, consilium: ConsiliumId, voter: UserId) -> bool {
	sqlx::query_scalar("SELECT burned_at IS NOT NULL FROM consilium_voter WHERE consilium_id = $1 AND user_id = $2")
		.bind(consilium.raw())
		.bind(voter.raw())
		.fetch_one(&h.pool)
		.await
		.unwrap()
}

/// Record an owner-roster change `seconds_ago` in the past, the way the lifecycle bridge
/// does when it applies a `ROLE_CHANGED` that adds or removes an owner.
async fn record_roster_change(h: &Harness, user: UserId, from_role: &str, to_role: &str, seconds_ago: i64) {
	sqlx::query("INSERT INTO governance_roster_change (user_id, from_role, to_role, changed_at) VALUES ($1, $2, $3, now() - make_interval(secs => $4))")
		.bind(user.raw())
		.bind(from_role)
		.bind(to_role)
		.bind(seconds_ago as f64)
		.execute(&h.pool)
		.await
		.unwrap();
}

async fn revenue_payout_count(h: &Harness) -> usize {
	h.withdrawals.list_revenue_payouts().await.unwrap().len()
}

#[tokio::test]
async fn the_threshold_is_more_than_half_of_all_owners_end_to_end() {
	let _lock = exclusive_governance().await;
	let Some(h) = harness().await else { return };

	// N=3 — threshold 2 over 2 voters: both peers must agree.
	reset_governance(&h).await;
	let roster = owners(&h, 3).await;
	let c = open_payout(&h, roster[0], "500").await;
	assert_eq!(c.consilium.owner_count(), 3);
	assert_eq!(c.consilium.threshold(), 2);
	assert_eq!(c.voters.len(), 2, "the initiator holds no seat");
	assert!(!vote(&h, c.consilium.id(), roster[1], VoteDecision::Approve).await.unwrap());
	assert_eq!(state_of(&h, c.consilium.id()).await, ConsiliumState::Open);
	assert!(vote(&h, c.consilium.id(), roster[2], VoteDecision::Approve).await.unwrap());
	assert_eq!(state_of(&h, c.consilium.id()).await, ConsiliumState::Approved);

	// N=4 — threshold 3 over 3 voters: unanimity among the peers.
	reset_governance(&h).await;
	let roster = owners(&h, 4).await;
	let c = open_payout(&h, roster[0], "500").await;
	assert_eq!(c.consilium.threshold(), 3);
	assert_eq!(c.voters.len(), 3);
	vote(&h, c.consilium.id(), roster[1], VoteDecision::Approve).await.unwrap();
	vote(&h, c.consilium.id(), roster[2], VoteDecision::Approve).await.unwrap();
	assert_eq!(state_of(&h, c.consilium.id()).await, ConsiliumState::Open, "two of three is short");
	vote(&h, c.consilium.id(), roster[3], VoteDecision::Approve).await.unwrap();
	assert_eq!(state_of(&h, c.consilium.id()).await, ConsiliumState::Approved);

	// N=5 — threshold 3 over 4 voters: 3 of 4, so one owner need not answer at all.
	reset_governance(&h).await;
	let roster = owners(&h, 5).await;
	let c = open_payout(&h, roster[0], "500").await;
	assert_eq!(c.consilium.threshold(), 3);
	assert_eq!(c.voters.len(), 4);
	vote(&h, c.consilium.id(), roster[1], VoteDecision::Approve).await.unwrap();
	vote(&h, c.consilium.id(), roster[2], VoteDecision::Approve).await.unwrap();
	assert_eq!(state_of(&h, c.consilium.id()).await, ConsiliumState::Open);
	assert!(vote(&h, c.consilium.id(), roster[3], VoteDecision::Approve).await.unwrap());
	assert_eq!(state_of(&h, c.consilium.id()).await, ConsiliumState::Approved);
	let final_view = consilium_app::find(h.consilia.as_ref(), c.consilium.id()).await.unwrap();
	assert_eq!(final_view.consilium.approvals(), 3);
	assert_eq!(
		final_view.voters.iter().filter(|v| v.decision == VoteDecision::Pending).count(),
		1,
		"the fourth seat never had to answer"
	);
}

#[tokio::test]
async fn a_fund_below_three_owners_cannot_open_a_payout_at_all() {
	let _lock = exclusive_governance().await;
	let Some(h) = harness().await else { return };
	reset_governance(&h).await;

	// Two owners: threshold 2, one eligible voter — arithmetically unreachable, so the
	// request is refused rather than stored as one that could never pass.
	let roster = owners(&h, 2).await;
	let err = consilium_app::open_revenue_payout(&ports(&h), roster[0], terms("500"), now()).await.unwrap_err();
	assert!(matches!(err, DomainError::Validation(_)), "expected an explicit refusal, got {err:?}");
	let stored: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM consilium WHERE state = 'open'").fetch_one(&h.pool).await.unwrap();
	assert_eq!(stored, 0, "nothing may be persisted for a quorum that can never be reached");
}

#[tokio::test]
async fn the_initiator_gets_no_token_and_cannot_vote_by_any_path() {
	let _lock = exclusive_governance().await;
	let Some(h) = harness().await else { return };
	reset_governance(&h).await;
	let roster = owners(&h, 3).await;
	let c = open_payout(&h, roster[0], "500").await;
	let id = c.consilium.id();

	// No seat row exists for them — the composite FK plus CHECK in `0025` make one
	// unrepresentable, so there is nothing for a vote to attach to.
	let seats: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM consilium_voter WHERE consilium_id = $1 AND user_id = $2")
		.bind(id.raw())
		.bind(roster[0].raw())
		.fetch_one(&h.pool)
		.await
		.unwrap();
	assert_eq!(seats, 0, "the initiator must have no seat");

	// No approval mail was addressed to them either, so no token was ever minted.
	let mails: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM consilium_mail WHERE consilium_id = $1 AND user_id = $2 AND kind = 'payout_approval'")
		.bind(id.raw())
		.bind(roster[0].raw())
		.fetch_one(&h.pool)
		.await
		.unwrap();
	assert_eq!(mails, 0, "the initiator must never be mailed an approval token");

	// The database refuses to manufacture one, even by direct insert: this is the property
	// that makes "the initiator cannot vote" structural rather than a forgettable check.
	let forged =
		sqlx::query("INSERT INTO consilium_voter (consilium_id, user_id, initiator_user_id, token_hash, code_hash, expires_at) VALUES ($1, $2, $2, $3, $4, now() + interval '1 day')")
			.bind(id.raw())
			.bind(roster[0].raw())
			.bind(vec![9u8; 32])
			.bind(vec![8u8; 32])
			.execute(&h.pool)
			.await;
	assert!(forged.is_err(), "the schema must refuse a seat for the initiator");

	// And the other two peers alone still carry it — the initiator is in the denominator.
	vote(&h, id, roster[1], VoteDecision::Approve).await.unwrap();
	vote(&h, id, roster[2], VoteDecision::Approve).await.unwrap();
	assert_eq!(state_of(&h, id).await, ConsiliumState::Approved);
}

#[tokio::test]
async fn only_one_consilium_may_be_open_at_a_time() {
	let _lock = exclusive_governance().await;
	let Some(h) = harness().await else { return };
	reset_governance(&h).await;
	let roster = owners(&h, 3).await;
	let first = open_payout(&h, roster[0], "500").await;

	// This is the whole of the concurrent-approval overdraw defence: two approved payouts
	// can never exist to race each other over the same revenue.
	let err = consilium_app::open_revenue_payout(&ports(&h), roster[0], terms("100"), now()).await.unwrap_err();
	assert!(matches!(err, DomainError::Conflict(_)), "expected a conflict, got {err:?}");

	// Closing the first frees the slot again.
	consilium_app::cancel(h.consilia.as_ref(), first.consilium.id(), roster[0], now()).await.unwrap();
	assert_eq!(state_of(&h, first.consilium.id()).await, ConsiliumState::Cancelled);
	let second = open_payout(&h, roster[0], "100").await;
	assert_eq!(second.consilium.state(), ConsiliumState::Open);
	// Votes are not carried over: the reopened request has its own hash and its own seats.
	assert_ne!(second.consilium.payload_hash_hex(), first.consilium.payload_hash_hex());
}

#[tokio::test]
async fn only_the_initiator_may_withdraw_their_own_consilium() {
	let _lock = exclusive_governance().await;
	let Some(h) = harness().await else { return };
	reset_governance(&h).await;
	let roster = owners(&h, 3).await;
	let c = open_payout(&h, roster[0], "500").await;
	let err = consilium_app::cancel(h.consilia.as_ref(), c.consilium.id(), roster[1], now()).await.unwrap_err();
	assert!(matches!(err, DomainError::Forbidden(_)));
	assert_eq!(state_of(&h, c.consilium.id()).await, ConsiliumState::Open);
}

#[tokio::test]
async fn five_wrong_codes_burn_the_token_and_a_burned_one_looks_unknown() {
	let _lock = exclusive_governance().await;
	let Some(h) = harness().await else { return };
	reset_governance(&h).await;
	let roster = owners(&h, 3).await;
	let c = open_payout(&h, roster[0], "500").await;
	let id = c.consilium.id();
	let (token, code) = credentials(&h, id, roster[1]).await;
	let audit = VoteAudit {
		client_ip: String::new(),
		user_agent: String::new(),
	};

	// The invitation is readable while the token is live.
	assert!(consilium_app::invitation(h.consilia.as_ref(), &token, now()).await.is_ok());

	for attempt in 1..MAX_CODE_ATTEMPTS {
		let err = consilium_app::submit_decision(h.consilia.as_ref(), &token, "WRONGCODE1", VoteDecision::Approve, &audit, now())
			.await
			.unwrap_err();
		// A wrong code is NOT the anonymous answer: the token is genuine and its holder
		// deserves to know they mistyped, and how many tries are left. `Validation` is what
		// carries that through the BFF as a 400 — `Forbidden` became an opaque 404, so the
		// count never reached the human and owners burned their own tokens finding out.
		assert!(matches!(err, DomainError::Validation(_)), "attempt {attempt} should be a refusal, got {err:?}");
		assert!(
			err.to_string().contains(&format!("{} attempts remaining", MAX_CODE_ATTEMPTS - attempt)),
			"the refusal must carry the remaining count, got {err:?}"
		);
	}
	// The fifth failure burns it.
	let burned = consilium_app::submit_decision(h.consilia.as_ref(), &token, "WRONGCODE1", VoteDecision::Approve, &audit, now())
		.await
		.unwrap_err();
	assert!(matches!(burned, DomainError::NotFound { .. }));

	// From here the token is indistinguishable from one that never existed — same variant,
	// same (empty) id, on both surfaces, and the CORRECT code no longer helps.
	let unknown_token = "0".repeat(64);
	let burned_read = consilium_app::invitation(h.consilia.as_ref(), &token, now()).await.unwrap_err();
	let unknown_read = consilium_app::invitation(h.consilia.as_ref(), &unknown_token, now()).await.unwrap_err();
	assert_eq!(burned_read.to_string(), unknown_read.to_string(), "a burned token must read exactly like an unknown one");
	let with_real_code = consilium_app::submit_decision(h.consilia.as_ref(), &token, &code, VoteDecision::Approve, &audit, now())
		.await
		.unwrap_err();
	assert_eq!(with_real_code.to_string(), unknown_read.to_string());

	// The burn is recorded, and every owner was told about it.
	let burn_notices: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM consilium_mail WHERE consilium_id = $1 AND kind = 'token_burned'")
		.bind(id.raw())
		.fetch_one(&h.pool)
		.await
		.unwrap();
	assert_eq!(burn_notices, 3, "the whole roster hears about a brute-force attempt");

	// The other peer's seat is untouched — one burned token does not disarm the consilium.
	assert!(consilium_app::invitation(h.consilia.as_ref(), &credentials(&h, id, roster[2]).await.0, now()).await.is_ok());
}

#[tokio::test]
async fn reading_an_invitation_never_costs_an_attempt() {
	let _lock = exclusive_governance().await;
	let Some(h) = harness().await else { return };
	reset_governance(&h).await;
	let roster = owners(&h, 3).await;
	let c = open_payout(&h, roster[0], "500").await;
	let (token, _) = credentials(&h, c.consilium.id(), roster[1]).await;

	// A corporate mail scanner fetches every URL in a message, often several times. If the
	// read counted against the attempt limit it would burn an owner's vote before they had
	// even opened the mail.
	for _ in 0..(MAX_CODE_ATTEMPTS + 3) {
		let view = consilium_app::invitation(h.consilia.as_ref(), &token, now()).await.unwrap();
		assert_eq!(view.attempts_remaining, MAX_CODE_ATTEMPTS as u32);
		assert_eq!(view.decision, VoteDecision::Pending);
	}
	let attempts: i32 = sqlx::query_scalar("SELECT attempts FROM consilium_voter WHERE consilium_id = $1 AND user_id = $2")
		.bind(c.consilium.id().raw())
		.bind(roster[1].raw())
		.fetch_one(&h.pool)
		.await
		.unwrap();
	assert_eq!(attempts, 0, "the read must be strictly side-effect free");
}

#[tokio::test]
async fn the_same_decision_twice_is_idempotent_and_a_different_one_is_refused() {
	let _lock = exclusive_governance().await;
	let Some(h) = harness().await else { return };
	reset_governance(&h).await;
	let roster = owners(&h, 5).await;
	let c = open_payout(&h, roster[0], "500").await;
	let id = c.consilium.id();

	assert!(!vote(&h, id, roster[1], VoteDecision::Approve).await.unwrap());
	// A retried request (a double-clicked button, an at-least-once edge) must not error and
	// must not count twice.
	assert!(!vote(&h, id, roster[1], VoteDecision::Approve).await.unwrap());
	let view = consilium_app::find(h.consilia.as_ref(), id).await.unwrap();
	assert_eq!(view.consilium.approvals(), 1, "the repeat is a no-op, not a second vote");

	// Changing your mind is not on offer — the vote is one-shot.
	let err = vote(&h, id, roster[1], VoteDecision::Reject).await.unwrap_err();
	assert!(matches!(err, DomainError::Conflict(_)), "expected a conflict, got {err:?}");
	let view = consilium_app::find(h.consilia.as_ref(), id).await.unwrap();
	assert_eq!(view.consilium.approvals(), 1);
	assert_eq!(view.consilium.rejections(), 0);
}

#[tokio::test]
async fn rejections_close_the_consilium_the_moment_the_threshold_is_unreachable() {
	let _lock = exclusive_governance().await;
	let Some(h) = harness().await else { return };

	// N=5: 4 voters, threshold 3 — the tally can afford exactly one refusal.
	reset_governance(&h).await;
	let roster = owners(&h, 5).await;
	let c = open_payout(&h, roster[0], "500").await;
	let id = c.consilium.id();
	assert!(!vote(&h, id, roster[1], VoteDecision::Reject).await.unwrap());
	assert_eq!(state_of(&h, id).await, ConsiliumState::Open, "three of the remaining four could still carry it");
	assert!(vote(&h, id, roster[2], VoteDecision::Reject).await.unwrap());
	assert_eq!(state_of(&h, id).await, ConsiliumState::Rejected, "only two voters remain and three are needed");

	// A rejected consilium takes no further votes and can never execute.
	let late = vote(&h, id, roster[3], VoteDecision::Approve).await.unwrap_err();
	assert!(matches!(late, DomainError::NotFound { .. }));
	assert!(consilium_app::execute(&ports(&h), id, now()).await.is_err());

	// N=3 needs both peers, so one refusal ends it at once.
	reset_governance(&h).await;
	let roster = owners(&h, 3).await;
	let c = open_payout(&h, roster[0], "500").await;
	assert!(vote(&h, c.consilium.id(), roster[1], VoteDecision::Reject).await.unwrap());
	assert_eq!(state_of(&h, c.consilium.id()).await, ConsiliumState::Rejected);
}

#[tokio::test]
async fn a_voter_who_lost_ownership_stops_counting_toward_quorum() {
	let _lock = exclusive_governance().await;
	let Some(h) = harness().await else { return };
	reset_governance(&h).await;
	// N=5: 4 voters, threshold 3, frozen at open and never recomputed.
	let roster = owners(&h, 5).await;
	let c = open_payout(&h, roster[0], "500").await;
	let id = c.consilium.id();

	vote(&h, id, roster[1], VoteDecision::Approve).await.unwrap();
	vote(&h, id, roster[2], VoteDecision::Approve).await.unwrap();
	assert_eq!(consilium_app::find(h.consilia.as_ref(), id).await.unwrap().consilium.approvals(), 2);

	// The first approver loses their seat. Their vote stays on the record but stops being
	// counted, so the roster change made approval HARDER — never easier.
	demote(&h, roster[1]).await;
	let view = consilium_app::find(h.consilia.as_ref(), id).await.unwrap();
	assert_eq!(view.consilium.approvals(), 1, "a vote from a seat that no longer exists does not count");
	assert_eq!(view.consilium.threshold(), 3, "the threshold is frozen — losing owners cannot lower the bar");
	assert!(
		view.voters.iter().any(|v| v.user_id == roster[1] && v.decision == VoteDecision::Approve),
		"the vote is still visible on the record"
	);

	// A third approval now brings the counted tally to only 2, so it stays open.
	vote(&h, id, roster[3], VoteDecision::Approve).await.unwrap();
	assert_eq!(state_of(&h, id).await, ConsiliumState::Open, "two counted approvals cannot reach a threshold of three");

	// The last remaining seat carries it — three approvals from three current owners.
	assert!(vote(&h, id, roster[4], VoteDecision::Approve).await.unwrap());
	assert_eq!(state_of(&h, id).await, ConsiliumState::Approved);
	assert_eq!(consilium_app::find(h.consilia.as_ref(), id).await.unwrap().consilium.approvals(), 3);
}

#[tokio::test]
async fn reaching_quorum_creates_exactly_one_payout_and_executing_twice_creates_no_second() {
	let _lock = exclusive_governance().await;
	let Some(h) = harness().await else { return };
	reset_governance(&h).await;
	fund_revenue(&h, "1000").await;
	let roster = owners(&h, 3).await;
	let c = open_payout(&h, roster[0], "500").await;
	let id = c.consilium.id();
	let before = revenue_payout_count(&h).await;

	vote(&h, id, roster[1], VoteDecision::Approve).await.unwrap();
	assert!(vote(&h, id, roster[2], VoteDecision::Approve).await.unwrap());
	// Approval alone moves no money — the payout is a separate, explicit step.
	assert_eq!(state_of(&h, id).await, ConsiliumState::Approved);
	assert_eq!(revenue_payout_count(&h).await, before, "quorum by itself must not create a payout");

	let executed = consilium_app::execute(&ports(&h), id, now()).await.unwrap();
	assert_eq!(executed.consilium.state(), ConsiliumState::Executed);
	assert_eq!(revenue_payout_count(&h).await, before + 1);

	// The payout id is derived from the consilium, so it is checkable rather than incidental.
	let expected = consilium_app::payout_id(id);
	assert_eq!(executed.consilium.executed_withdrawal_id(), Some(expected));
	let payout = h.withdrawals.find_by_id(expected).await.unwrap().expect("the payout exists under the derived id");
	assert!(payout.source().is_revenue());
	assert_eq!(payout.amount(), usdt("500"));
	assert_eq!(payout.address().as_str(), PAYOUT_ADDRESS);

	// Re-executing — the sweeper's retry, or a redelivered call — must be a no-op. This is
	// the difference between an at-least-once execution path and a double payout.
	for _ in 0..3 {
		let again = consilium_app::execute(&ports(&h), id, now()).await.unwrap();
		assert_eq!(again.consilium.executed_withdrawal_id(), Some(expected));
	}
	assert_eq!(revenue_payout_count(&h).await, before + 1, "a retried execution must never open a second payout");
	h.relay.drain().await;
}

#[tokio::test]
async fn a_payout_the_revenue_no_longer_covers_lands_in_execution_failed() {
	let _lock = exclusive_governance().await;
	let Some(h) = harness().await else { return };
	reset_governance(&h).await;
	let roster = owners(&h, 3).await;

	// Open against revenue that exists...
	let available = h.ledger.balance(&LedgerAccountKey::FeeRevenue).await.unwrap().available();
	let c = open_payout(&h, roster[0], "500").await;
	let id = c.consilium.id();
	vote(&h, id, roster[1], VoteDecision::Approve).await.unwrap();
	vote(&h, id, roster[2], VoteDecision::Approve).await.unwrap();
	assert_eq!(state_of(&h, id).await, ConsiliumState::Approved);

	// ...then drain it away before execution, exactly as a competing spend would.
	drain_revenue(&h, available).await;

	let failed = consilium_app::execute(&ports(&h), id, now()).await.unwrap();
	assert_eq!(failed.consilium.state(), ConsiliumState::ExecutionFailed);
	assert!(
		failed.consilium.failure_reason().unwrap_or_default().contains("revenue"),
		"the owners must be able to read WHY: {:?}",
		failed.consilium.failure_reason()
	);
	// Terminal — nothing retries silently, so the sweeper will not pick it up again.
	assert!(!h.consilia.awaiting_execution().await.unwrap().contains(&id));
	assert!(consilium_app::execute(&ports(&h), id, now()).await.is_err());

	// Put the fund's revenue back. `fee` is a global singleton shared with the other
	// suites, so a test that empties it restores it rather than leaving every sibling
	// looking at a fund that has never earned anything.
	fund_revenue(&h, &Usdt::from_base_units(available).to_decimal_string()).await;
}

#[tokio::test]
async fn an_expired_consilium_can_never_execute_however_late_a_vote_arrives() {
	let _lock = exclusive_governance().await;
	let Some(h) = harness().await else { return };
	reset_governance(&h).await;
	fund_revenue(&h, "1000").await;
	let roster = owners(&h, 3).await;
	let c = open_payout(&h, roster[0], "500").await;
	let id = c.consilium.id();
	vote(&h, id, roster[1], VoteDecision::Approve).await.unwrap();
	let before = revenue_payout_count(&h).await;

	expire_the_window(&h, id).await;

	// The token's own deadline refuses a late vote even BEFORE the sweeper has run, which is
	// what makes this hold in the window between expiry and the next sweep.
	let late = vote(&h, id, roster[2], VoteDecision::Approve).await.unwrap_err();
	assert!(matches!(late, DomainError::NotFound { .. }), "a vote past the deadline must not be accepted");
	assert_eq!(state_of(&h, id).await, ConsiliumState::Open, "still open — only the sweeper closes it");

	// The sweeper closes it, and the vote that never landed cannot have carried it.
	assert_eq!(consilium_app::sweep_expired(h.consilia.as_ref(), now()).await.unwrap(), 1);
	assert_eq!(state_of(&h, id).await, ConsiliumState::Expired);
	// Execution is reachable only from `approved`; expiry only from `open`. No ordering of
	// the two can produce a payout from a dead request.
	assert!(consilium_app::execute(&ports(&h), id, now()).await.is_err());
	assert_eq!(revenue_payout_count(&h).await, before, "an expired consilium must move no money");
	assert!(consilium_app::sweep_expired(h.consilia.as_ref(), now()).await.unwrap() == 0, "the sweep is idempotent");
}

#[tokio::test]
async fn editing_the_terms_after_approval_cannot_spend_the_approval() {
	let _lock = exclusive_governance().await;
	let Some(h) = harness().await else { return };
	reset_governance(&h).await;
	fund_revenue(&h, "5000").await;
	let roster = owners(&h, 3).await;
	let c = open_payout(&h, roster[0], "500").await;
	let id = c.consilium.id();
	vote(&h, id, roster[1], VoteDecision::Approve).await.unwrap();
	vote(&h, id, roster[2], VoteDecision::Approve).await.unwrap();
	assert_eq!(state_of(&h, id).await, ConsiliumState::Approved);
	let before = revenue_payout_count(&h).await;

	// There is no edit RPC, so this is a direct tamper with the stored row — the threat the
	// payload hash exists for. An approval is a signature over those exact terms.
	sqlx::query("UPDATE consilium SET terms = jsonb_set(terms, '{amount}', to_jsonb($2::text)) WHERE id = $1")
		.bind(id.raw())
		.bind(usdt("4000").base_units().to_string())
		.execute(&h.pool)
		.await
		.unwrap();

	let result = consilium_app::execute(&ports(&h), id, now()).await.unwrap();
	assert_eq!(result.consilium.state(), ConsiliumState::ExecutionFailed);
	assert!(result.consilium.failure_reason().unwrap_or_default().contains("payload hash"));
	assert_eq!(revenue_payout_count(&h).await, before, "tampered terms must never reach the money plane");
}

#[tokio::test]
async fn an_impossible_payout_is_refused_at_open_not_after_a_72h_vote() {
	let _lock = exclusive_governance().await;
	let Some(h) = harness().await else { return };
	reset_governance(&h).await;
	let roster = owners(&h, 3).await;
	let revenue = Usdt::from_base_units(h.ledger.balance(&LedgerAccountKey::FeeRevenue).await.unwrap().available());

	// More than the fund has ever earned: refused now, rather than after three owners have
	// spent three days approving something that could never have shipped.
	let beyond = revenue.checked_add(usdt("1000000")).unwrap();
	let too_much = RevenuePayoutTerms::new(Network::Bep20, WalletAddress::parse(Network::Bep20, PAYOUT_ADDRESS).unwrap(), beyond, String::new()).unwrap();
	let err = consilium_app::open_revenue_payout(&ports(&h), roster[0], too_much, now()).await.unwrap_err();
	assert!(matches!(err, DomainError::Validation(_)), "got {err:?}");

	// Below the per-network minimum — the same shape gate the payout itself applies.
	let dust = RevenuePayoutTerms::new(Network::Bep20, WalletAddress::parse(Network::Bep20, PAYOUT_ADDRESS).unwrap(), usdt("1"), String::new()).unwrap();
	assert!(consilium_app::open_revenue_payout(&ports(&h), roster[0], dust, now()).await.is_err());

	let stored: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM consilium").fetch_one(&h.pool).await.unwrap();
	let open: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM consilium WHERE state = 'open'").fetch_one(&h.pool).await.unwrap();
	assert_eq!(open, 0, "a refused request leaves nothing open (of {stored} historical rows)");
}

#[tokio::test]
async fn every_eligible_seat_is_mailed_a_distinct_token_and_code() {
	let _lock = exclusive_governance().await;
	let Some(h) = harness().await else { return };
	reset_governance(&h).await;
	let roster = owners(&h, 4).await;
	let c = open_payout(&h, roster[0], "500").await;
	let id = c.consilium.id();

	let mut tokens = Vec::new();
	let mut codes = Vec::new();
	for voter in &roster[1..] {
		let (token, code) = credentials(&h, id, *voter).await;
		// 32 random bytes hex; 10 symbols from Crockford base32 minus I, L, O and U.
		assert_eq!(token.len(), 64, "a token is 32 random bytes, well past the 24-byte floor");
		assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
		assert_eq!(code.len(), 10);
		assert!(code.chars().all(|c| "0123456789ABCDEFGHJKMNPQRSTVWXYZ".contains(c)), "code {code} left the unambiguous alphabet");
		assert!(!code.contains(['I', 'L', 'O', 'U']), "the four misread glyphs must never appear");
		tokens.push(token);
		codes.push(code);
	}
	tokens.sort();
	tokens.dedup();
	codes.sort();
	codes.dedup();
	assert_eq!(tokens.len(), 3, "each seat gets its own token");
	assert_eq!(codes.len(), 3, "each seat gets its own code");

	// Only digests are stored: a dump of the seat table yields nothing that can vote.
	let rows = sqlx::query("SELECT token_hash, code_hash FROM consilium_voter WHERE consilium_id = $1")
		.bind(id.raw())
		.fetch_all(&h.pool)
		.await
		.unwrap();
	assert_eq!(rows.len(), 3);
	for row in &rows {
		let token_hash: Vec<u8> = row.try_get("token_hash").unwrap();
		let code_hash: Vec<u8> = row.try_get("code_hash").unwrap();
		assert_eq!(token_hash.len(), 32);
		assert_eq!(code_hash.len(), 32);
		assert!(!tokens.iter().any(|t| t.as_bytes() == token_hash), "the plaintext token must not be what is stored");
	}

	// One seat's token cannot answer for another seat.
	let (first_token, _) = credentials(&h, id, roster[1]).await;
	let (_, second_code) = credentials(&h, id, roster[2]).await;
	let audit = VoteAudit {
		client_ip: String::new(),
		user_agent: String::new(),
	};
	assert!(
		consilium_app::submit_decision(h.consilia.as_ref(), &first_token, &second_code, VoteDecision::Approve, &audit, now())
			.await
			.is_err(),
		"a code only works with the token it was minted beside"
	);
}

/// PITFALL 10 vs. THE MISTYPED CODE. These are two different secrets and they get two
/// different answers. The TOKEN is protected by indistinguishability — unknown, expired,
/// spent and burned must all read identically, or the endpoint becomes an oracle for which
/// consilia exist. The CODE is protected by the five-attempt ceiling, and someone holding a
/// live token already knows it exists, so naming a wrong code reveals nothing new.
#[tokio::test]
async fn a_wrong_code_and_an_unknown_token_are_told_apart_while_the_dead_token_states_stay_identical() {
	let _lock = exclusive_governance().await;
	let Some(h) = harness().await else { return };
	reset_governance(&h).await;
	let roster = owners(&h, 3).await;
	let c = open_payout(&h, roster[0], "500").await;
	let id = c.consilium.id();
	let (token, code) = credentials(&h, id, roster[1]).await;
	let audit = VoteAudit {
		client_ip: String::new(),
		user_agent: String::new(),
	};

	let wrong_code = consilium_app::submit_decision(h.consilia.as_ref(), &token, "WRONGCODE1", VoteDecision::Approve, &audit, now())
		.await
		.unwrap_err();
	let unknown_token = consilium_app::submit_decision(h.consilia.as_ref(), &"0".repeat(64), &code, VoteDecision::Approve, &audit, now())
		.await
		.unwrap_err();
	assert!(matches!(wrong_code, DomainError::Validation(_)), "a mistyped code is a 400, not a 404: {wrong_code:?}");
	assert!(
		matches!(unknown_token, DomainError::NotFound { .. }),
		"an unknown token is the anonymous answer: {unknown_token:?}"
	);
	assert_ne!(
		wrong_code.to_string(),
		unknown_token.to_string(),
		"an owner who mistyped must not be told their invitation does not exist"
	);

	// ...while every way a TOKEN can be dead still reads the same.
	let expired = consilium_app::submit_decision(h.consilia.as_ref(), &token, &code, VoteDecision::Approve, &audit, now() + 100 * 3600)
		.await
		.unwrap_err();
	assert_eq!(expired.to_string(), unknown_token.to_string(), "an expired token reads like an unknown one");

	// Spend the seat, then re-read: a spent token is still indistinguishable on the
	// side-effect-free surface.
	vote(&h, id, roster[1], VoteDecision::Approve).await.unwrap();
	let spent_read = consilium_app::invitation(h.consilia.as_ref(), &token, now()).await.unwrap_err();
	let unknown_read = consilium_app::invitation(h.consilia.as_ref(), &"0".repeat(64), now()).await.unwrap_err();
	assert_eq!(spent_read.to_string(), unknown_read.to_string(), "a spent token reads like an unknown one");
}

/// THE ATTEMPT COUNTER MUST NOT OVERFLOW ITS OWN CHECK.
///
/// `CHECK (attempts <= 5)` and an unconditional increment are incompatible: a seat that
/// answered correctly on its fifth try sits at exactly 5 un-burned (the correct-code path
/// never burns), and the next POST — a double-click, a gateway retry — pushed it to 6,
/// violated the constraint, and turned that seat's every future request into a 503. Both
/// halves are pinned here: the retry stays idempotent, and the column stays legal.
#[tokio::test]
async fn a_seat_that_already_answered_spends_no_further_attempts() {
	let _lock = exclusive_governance().await;
	let Some(h) = harness().await else { return };
	reset_governance(&h).await;
	let roster = owners(&h, 5).await;
	let c = open_payout(&h, roster[0], "500").await;
	let id = c.consilium.id();
	let (token, code) = credentials(&h, id, roster[1]).await;
	let audit = VoteAudit {
		client_ip: String::new(),
		user_agent: String::new(),
	};

	// Walk the seat to the very edge: four wrong guesses, then the correct code.
	for _ in 1..MAX_CODE_ATTEMPTS {
		assert!(
			consilium_app::submit_decision(h.consilia.as_ref(), &token, "WRONGCODE1", VoteDecision::Approve, &audit, now())
				.await
				.is_err()
		);
	}
	assert!(
		consilium_app::submit_decision(h.consilia.as_ref(), &token, &code, VoteDecision::Approve, &audit, now())
			.await
			.is_ok()
	);
	assert_eq!(attempts_of(&h, id, roster[1]).await, MAX_CODE_ATTEMPTS, "the seat sits at the ceiling, correct and un-burned");

	// The double-click. Under the old unconditional increment this wrote 6 and every later
	// request from this seat became a 503 forever.
	for _ in 0..3 {
		let retry = consilium_app::submit_decision(h.consilia.as_ref(), &token, &code, VoteDecision::Approve, &audit, now()).await;
		assert!(retry.is_ok(), "a retried submission from a seat that already answered must stay idempotent, got {retry:?}");
	}
	assert_eq!(
		attempts_of(&h, id, roster[1]).await,
		MAX_CODE_ATTEMPTS,
		"an answered seat is charged nothing, and the column never exceeds its CHECK"
	);
	assert!(!burned_of(&h, id, roster[1]).await, "an answered seat is not burned by its own retries");

	// A wrong code against the answered seat is refused, still without burning it.
	let wrong = consilium_app::submit_decision(h.consilia.as_ref(), &token, "WRONGCODE1", VoteDecision::Approve, &audit, now())
		.await
		.unwrap_err();
	assert!(matches!(wrong, DomainError::Validation(_)), "got {wrong:?}");
	assert!(!burned_of(&h, id, roster[1]).await, "nothing was counted, so nothing can burn");
	assert_eq!(attempts_of(&h, id, roster[1]).await, MAX_CODE_ATTEMPTS);
}

/// The mechanism is inert without a mailer, so it refuses to pretend otherwise: opening a
/// consilium nobody could be sent a token for is a consilium that expires unvotable 72h
/// later, having looked healthy the whole time.
#[tokio::test]
async fn opening_without_a_governance_mailer_is_refused() {
	let _lock = exclusive_governance().await;
	let Some(h) = harness().await else { return };
	reset_governance(&h).await;
	let roster = owners(&h, 3).await;
	let unwired = consilium_app::ConsiliumPorts {
		governance_mail_wired: false,
		..ports(&h)
	};
	let err = consilium_app::open_revenue_payout(&unwired, roster[0], terms("500"), now()).await.unwrap_err();
	assert!(matches!(err, DomainError::Conflict(_)), "got {err:?}");
	assert!(err.to_string().contains("governance mail is not configured"), "the refusal must name the cause: {err}");
}

/// THE COOLING-OFF PERIOD, FIRST HALF: a fresh roster change refuses a NEW proposal.
///
/// This does not stop a majority from seizing the roster — nothing can; a majority owns the
/// roster by definition. It stops the seizure and the payout from being one uninterrupted
/// motion, which is the part an auditor or a remaining honest owner can actually act on.
#[tokio::test]
async fn a_recent_owner_roster_change_freezes_new_payout_proposals() {
	let _lock = exclusive_governance().await;
	let Some(h) = harness().await else { return };
	reset_governance(&h).await;
	let roster = owners(&h, 4).await;

	// A seat changed hands an hour ago.
	record_roster_change(&h, roster[3], "investor", "owner", 3600).await;
	let err = consilium_app::open_revenue_payout(&ports(&h), roster[0], terms("500"), now()).await.unwrap_err();
	assert!(matches!(err, DomainError::Conflict(_)), "got {err:?}");
	assert!(err.to_string().contains("cooling-off"), "the refusal must name the cooling-off period: {err}");
	assert!(err.to_string().contains("lifts in"), "and say when it lifts: {err}");

	// Once the window has passed, the same proposal opens normally.
	sqlx::query("DELETE FROM governance_roster_change").execute(&h.pool).await.unwrap();
	record_roster_change(&h, roster[3], "investor", "owner", consilium_app::ROSTER_COOLING_OFF_SECS + 60).await;
	assert!(
		consilium_app::open_revenue_payout(&ports(&h), roster[0], terms("500"), now()).await.is_ok(),
		"a settled roster does not block a payout forever"
	);
}

/// THE COOLING-OFF PERIOD, SECOND HALF: a change landing on an ALREADY-OPEN proposal voids
/// it. Without this the window is trivially straddled — open the request first, seize the
/// roster after, and the freeze on new proposals never applies.
#[tokio::test]
async fn an_owner_roster_change_voids_a_payout_request_that_was_already_open() {
	let _lock = exclusive_governance().await;
	let Some(h) = harness().await else { return };
	reset_governance(&h).await;
	let roster = owners(&h, 5).await;
	let c = open_payout(&h, roster[0], "500").await;
	let id = c.consilium.id();
	vote(&h, id, roster[1], VoteDecision::Approve).await.unwrap();
	assert_eq!(state_of(&h, id).await, ConsiliumState::Open);

	// The roster moves while the request is live.
	record_roster_change(&h, roster[4], "owner", "investor", 0).await;
	let voided = h.consilia.void_open_for_roster_change(now(), now()).await.unwrap();
	assert_eq!(voided, 1, "the open request is voided");
	assert_eq!(state_of(&h, id).await, ConsiliumState::Cancelled);

	// The audience is told WHY, so the delay reads as the deliberate control it is.
	let told: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM consilium_mail WHERE consilium_id = $1 AND payload::text LIKE '%roster changed%'")
		.bind(id.raw())
		.fetch_one(&h.pool)
		.await
		.unwrap();
	assert!(told > 0, "the owners are told why their request disappeared");

	// Idempotent: a second sweep voids nothing more.
	assert_eq!(h.consilia.void_open_for_roster_change(now(), now()).await.unwrap(), 0);
}

/// PITFALL 1 AT THE MOMENT THAT SPENDS MONEY. The stored `approved` state is a fact about
/// the tally when the carrying vote landed; execution can happen much later. An owner who
/// loses their seat in between must take their approval with them — otherwise the invariant
/// holds everywhere except the one place it matters.
#[tokio::test]
async fn an_approval_invalidated_by_a_roster_change_is_refused_at_execution() {
	let _lock = exclusive_governance().await;
	let Some(h) = harness().await else { return };
	reset_governance(&h).await;
	// N=3: threshold 2, two eligible voters — so both must approve, and losing either one
	// drops the live tally below the frozen bar.
	let roster = owners(&h, 3).await;
	let c = open_payout(&h, roster[0], "500").await;
	let id = c.consilium.id();
	vote(&h, id, roster[1], VoteDecision::Approve).await.unwrap();
	assert!(vote(&h, id, roster[2], VoteDecision::Approve).await.unwrap());
	assert_eq!(state_of(&h, id).await, ConsiliumState::Approved);

	// Simulate the crash-before-execution window, then remove a seat that had approved.
	sqlx::query("UPDATE consilium SET state = 'approved', executed_withdrawal_id = NULL WHERE id = $1")
		.bind(id.raw())
		.execute(&h.pool)
		.await
		.unwrap();
	demote(&h, roster[2]).await;

	let before = revenue_payout_count(&h).await;
	let view = consilium_app::execute(&ports(&h), id, now()).await.unwrap();
	assert_eq!(view.consilium.state(), ConsiliumState::ExecutionFailed, "a stale quorum must not spend money");
	assert!(
		view.consilium.failure_reason().unwrap_or_default().contains("still held by current owners"),
		"the reason must name the roster change: {:?}",
		view.consilium.failure_reason()
	);
	assert_eq!(revenue_payout_count(&h).await, before, "no payout was created");
}

/// THE TWO-CALLER RACE. The inline execute after the carrying vote and the sweeper both see
/// "no payout under this id" and both try to create one. One wins on the `withdrawals`
/// primary key; the loser must record the payout that ACTUALLY EXISTS, not a phantom
/// failure. Recording `Failed` there is a lie that sticks: the owners are mailed a failure,
/// `awaiting_execution` never returns the consilium again, and the payout is broadcast
/// anyway.
#[tokio::test]
async fn two_concurrent_executions_agree_on_one_payout_and_neither_records_a_phantom_failure() {
	let _lock = exclusive_governance().await;
	let Some(h) = harness().await else { return };
	reset_governance(&h).await;
	let roster = owners(&h, 3).await;
	let c = open_payout(&h, roster[0], "500").await;
	let id = c.consilium.id();
	vote(&h, id, roster[1], VoteDecision::Approve).await.unwrap();
	assert!(vote(&h, id, roster[2], VoteDecision::Approve).await.unwrap());

	// Put it back to `approved` with no payout recorded, the state a crash between the
	// verdict and the money leaves behind — then drive BOTH callers at once.
	sqlx::query("UPDATE consilium SET state = 'approved', executed_withdrawal_id = NULL WHERE id = $1")
		.bind(id.raw())
		.execute(&h.pool)
		.await
		.unwrap();
	sqlx::query("DELETE FROM withdrawals WHERE id = $1")
		.bind(consilium_app::payout_id(id).raw())
		.execute(&h.pool)
		.await
		.unwrap();

	let before = revenue_payout_count(&h).await;
	let (a, b) = (ports(&h), ports(&h));
	let (first, second) = tokio::join!(consilium_app::execute(&a, id, now()), consilium_app::execute(&b, id, now()));
	let first = first.expect("the first execution must not error");
	let second = second.expect("the second execution must not error");

	for view in [&first, &second] {
		assert_eq!(
			view.consilium.state(),
			ConsiliumState::Executed,
			"a lost race is not a failure — reason: {:?}",
			view.consilium.failure_reason()
		);
	}
	assert_eq!(revenue_payout_count(&h).await, before + 1, "exactly one payout, however many callers raced");
	assert_eq!(state_of(&h, id).await, ConsiliumState::Executed);
}

/// THE SPEC TABLE IN docs/CONSILIUM.md, PINNED. Both planes implement an emailed-token flow
/// and had already drifted on these two points before they were decided. This is banking's
/// side of the table, asserted rather than assumed.
#[tokio::test]
async fn the_shared_token_specification_holds_on_this_side() {
	let _lock = exclusive_governance().await;
	let Some(h) = harness().await else { return };
	reset_governance(&h).await;
	let roster = owners(&h, 5).await;
	let c = open_payout(&h, roster[0], "500").await;
	let id = c.consilium.id();
	let (token, code) = credentials(&h, id, roster[1]).await;
	let audit = VoteAudit {
		client_ip: String::new(),
		user_agent: String::new(),
	};

	// "Reading an invitation costs nothing."
	assert!(consilium_app::invitation(h.consilia.as_ref(), &token, now()).await.is_ok());
	assert_eq!(attempts_of(&h, id, roster[1]).await, 0, "reading must never consume an attempt");

	// "Correct code: the attempt counter is NOT reset — a token that has been guessed at
	// stays closer to burning."
	assert!(
		consilium_app::submit_decision(h.consilia.as_ref(), &token, "WRONGCODE1", VoteDecision::Approve, &audit, now())
			.await
			.is_err()
	);
	assert!(
		consilium_app::submit_decision(h.consilia.as_ref(), &token, "WRONGCODE2", VoteDecision::Approve, &audit, now())
			.await
			.is_err()
	);
	assert_eq!(attempts_of(&h, id, roster[1]).await, 2);
	assert!(
		consilium_app::submit_decision(h.consilia.as_ref(), &token, &code, VoteDecision::Approve, &audit, now())
			.await
			.is_ok()
	);
	// 3, not 2: the successful submission is itself a counted attempt, and — the point of
	// the rule — the two guesses before it are NOT forgiven. A reset here would let an
	// attacker refresh the budget at will against any seat whose code they eventually find.
	assert_eq!(attempts_of(&h, id, roster[1]).await, 3, "a correct code does not reset the counter");

	// "Token against an already-closed request: refused BEFORE an attempt is counted, so a
	// mail scanner cannot spend a human's attempt budget."
	let (other_token, _) = credentials(&h, id, roster[2]).await;
	let before = attempts_of(&h, id, roster[2]).await;
	consilium_app::cancel(h.consilia.as_ref(), id, roster[0], now()).await.unwrap();
	let closed = consilium_app::submit_decision(h.consilia.as_ref(), &other_token, "WRONGCODE1", VoteDecision::Approve, &audit, now())
		.await
		.unwrap_err();
	assert!(matches!(closed, DomainError::NotFound { .. }), "a closed request answers anonymously: {closed:?}");
	assert_eq!(attempts_of(&h, id, roster[2]).await, before, "a closed request must not charge an attempt");
}
