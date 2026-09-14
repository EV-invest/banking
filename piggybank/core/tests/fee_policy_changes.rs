//! Integration tests for changing a fund's fee terms (#233) — real Postgres **and**
//! TigerBeetle (no mocks, per the project rules). They run when `DATABASE_URL` is set and a
//! TigerBeetle replica is reachable (`nix run .#db` + `.#tb`), and skip otherwise.
//!
//! What is pinned here is the LIFE of a change rather than the arithmetic of a fee (that is
//! `tests/fee_policy.rs`): the notice period binds exactly while someone holds units; a
//! tightening beyond the house envelope needs an owner to propose it and the owners to carry
//! it; every holder is queued one notice when a change becomes scheduled; a refused or
//! withdrawn quorum closes the change; one change is on its way per product at a time; and a
//! promotion settles the elapsed window at the OLD rate before the new one binds.
//!
//! The owner roster is global (`users.role = 'owner'`), so every test takes
//! [`exclusive`] and starts from a cleared roster; products, holders and changes are all
//! per test.

use std::sync::Arc;

use domain::{
	allocations::{Allocation, AllocationAccess, AllocationIcon, AllocationId},
	auth::AuthSubject,
	balance::{LedgerAccountKey, Party, ServiceId},
	consilium::{ConsiliumId, ConsiliumState, VoteDecision},
	error::DomainError,
	fees::{ChangeRequirement, CrystallizationPeriod, FeePolicy, FeePolicyChangeId, FeePolicyChangeState, MAX_EFFECTIVE_FROM_HORIZON_SECS, MIN_NOTICE_SECS, ManagementBasis, Trigger},
	money::{Network, Shares, TxRef, Usdt},
	users::{Email, UserId},
};
use piggybank_core::{
	application::{balance as balance_app, consilium as consilium_app, fees as fee_app, funds as funds_app},
	config::KycGate,
	infrastructure::{
		allocations::PgAllocations,
		consilium::PgConsilia,
		custody::StubCustody,
		deposits::PgDeposits,
		fee_policy_changes::PgFeePolicyChanges,
		fees::{PgFeeAssessments, PgFeePolicies, PgPositionAccruals},
		nav::PgNav,
		outflow::PgOutflowPolicy,
		payments::PgPayments,
		relay::Relay,
		subscriptions::PgSubscriptions,
		users::PgUsers,
		withdrawals::PgWithdrawals,
	},
	ports::{
		AllocationRegistry, ConsiliumRepository, PaymentRepository, UserRepository, WithdrawalRepository,
		consilium::VoteAudit,
		fees::{FeePolicies, FeePolicyChange, FeePolicyChanges, NewFeePolicyChange, PositionAccruals},
		ledger::Ledger,
	},
};
use sqlx::PgPool;
use tokio::sync::Notify;
use uuid::Uuid;

mod common;

const YEAR: i64 = 365 * 24 * 60 * 60;
const APPROVAL_URL_BASE: &str = "https://example.test/approve";
const CONSENT_URL_BASE: &str = "https://example.test/consent";
const CONFIGURED: [Network; 1] = [Network::Bep20];
/// Slack for the second or two between a test's `now()` and the one the write path stamps.
const CLOCK_SLACK: i64 = 30;

static EXCLUSIVE: std::sync::LazyLock<tokio::sync::Mutex<()>> = std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

async fn exclusive() -> tokio::sync::MutexGuard<'static, ()> {
	EXCLUSIVE.lock().await
}

struct Harness {
	pool: PgPool,
	allocations: PgAllocations,
	subs: PgSubscriptions,
	nav: PgNav,
	deposits: PgDeposits,
	policies: PgFeePolicies,
	changes: PgFeePolicyChanges,
	accruals: PgPositionAccruals,
	assessments: PgFeeAssessments,
	consilia: Arc<dyn ConsiliumRepository>,
	withdrawals: Arc<dyn WithdrawalRepository>,
	payments: Arc<dyn PaymentRepository>,
	users: Arc<dyn UserRepository>,
	outflow: PgOutflowPolicy,
	ledger: Arc<dyn Ledger>,
	relay: Relay,
	notify: Arc<Notify>,
}

async fn harness() -> Option<Harness> {
	let pool = common::pool().await?;
	let ledger = common::seeded_ledger(&pool, "fee-policy-change test").await?;
	let notify = Arc::new(Notify::new());
	Some(Harness {
		allocations: PgAllocations::new(pool.clone()),
		subs: PgSubscriptions::new(pool.clone()),
		nav: PgNav::new(pool.clone()),
		deposits: PgDeposits::new(pool.clone()),
		policies: PgFeePolicies::new(pool.clone()),
		changes: PgFeePolicyChanges::new(pool.clone()),
		accruals: PgPositionAccruals::new(pool.clone()),
		assessments: PgFeeAssessments::new(pool.clone()),
		consilia: Arc::new(PgConsilia::new(pool.clone())),
		withdrawals: Arc::new(PgWithdrawals::new(pool.clone())),
		payments: Arc::new(PgPayments::new(pool.clone())),
		users: Arc::new(PgUsers::new(pool.clone())),
		outflow: PgOutflowPolicy::new(pool.clone()),
		relay: Relay::new(pool.clone(), ledger.clone(), Arc::new(StubCustody), notify.clone()),
		ledger,
		notify,
		pool,
	})
}

fn policy_ports(h: &Harness) -> fee_app::FeePolicyPorts<'_> {
	fee_app::FeePolicyPorts {
		policies: &h.policies,
		changes: &h.changes,
		allocations: &h.allocations,
		consilia: h.consilia.as_ref(),
		approval_url_base: APPROVAL_URL_BASE,
		governance_mail_wired: true,
	}
}

fn consilium_ports(h: &Harness) -> consilium_app::ConsiliumPorts<'_> {
	consilium_app::ConsiliumPorts {
		consilia: h.consilia.as_ref(),
		withdrawals: h.withdrawals.as_ref(),
		payments: h.payments.as_ref(),
		users: h.users.as_ref(),
		ledger: h.ledger.as_ref(),
		custody: &StubCustody,
		policy: &h.outflow,
		allocations: &h.allocations,
		fee_changes: &h.changes,
		relay: &h.notify,
		configured: &CONFIGURED,
		kyc: KycGate::LIFTED,
		approval_url_base: APPROVAL_URL_BASE,
		consent_url_base: CONSENT_URL_BASE,
		governance_mail_wired: true,
	}
}

fn fund_ports(h: &Harness) -> funds_app::FundPorts<'_> {
	funds_app::FundPorts {
		allocations: &h.allocations,
		ledger: h.ledger.as_ref(),
		nav: &h.nav,
		relay: &h.notify,
	}
}

fn now() -> i64 {
	std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

fn usdt(decimal: &str) -> Usdt {
	Usdt::parse_decimal(decimal).unwrap()
}

fn policy(management: u32, performance: u32, hurdle: u32, basis: ManagementBasis, period: CrystallizationPeriod) -> FeePolicy {
	FeePolicy::new(management, performance, hurdle, basis, period).unwrap()
}

/// The house terms with the management rate raised past the envelope — the change that
/// needs the owners.
fn dearer() -> FeePolicy {
	policy(300, 2_000, 0, ManagementBasis::InvestedCapital, CrystallizationPeriod::Annual)
}

fn unique_service() -> ServiceId {
	ServiceId::parse(&format!("fpc-{}", Uuid::new_v4())).unwrap()
}

/// A registered, open product with NO policy yet — every test decides its own terms.
async fn open_fund(h: &Harness, service: &ServiceId) {
	let mut allocation = Allocation::register(AllocationId::new(), service.clone(), "EV Trading", "Systematic crypto", AllocationIcon::default()).unwrap();
	h.allocations.register(&mut allocation).await.unwrap();
	h.allocations.open(service).await.unwrap();
	h.allocations.set_access(service, AllocationAccess::Invest).await.unwrap();
}

async fn schedule(h: &Harness, requester: UserId, service: &ServiceId, policy: FeePolicy, effective_from: i64, reason: &str) -> Result<FeePolicyChange, DomainError> {
	fee_app::schedule_policy(
		&policy_ports(h),
		requester,
		fee_app::PolicyChangeRequest {
			service: service.clone(),
			policy,
			requested_effective_from_unix: effective_from,
			reason: reason.to_owned(),
		},
		now(),
	)
	.await
}

/// Install terms on a fund with no holders: scheduled and promoted in one breath.
async fn install(h: &Harness, service: &ServiceId, policy: FeePolicy) {
	let change = schedule(h, UserId::new(), service, policy, 0, "").await.unwrap();
	assert!(h.changes.promote(change.id, now()).await.unwrap());
	assert_eq!(h.policies.find(service).await.unwrap(), Some(policy));
}

/// A provisioned investor with a mirrored identity-plane id, so a notice can be addressed.
async fn investor(h: &Harness) -> UserId {
	let tag = Uuid::new_v4();
	let id = h
		.users
		.provision(
			AuthSubject::parse(&format!("fpc-{tag}")).unwrap(),
			Email::parse(&format!("fpc-{}@example.test", tag.simple())).unwrap(),
			true,
		)
		.await
		.unwrap()
		.id();
	sqlx::query("UPDATE users SET concierge_user_id = $2 WHERE id = $1")
		.bind(id.raw())
		.bind(Uuid::new_v4())
		.execute(&h.pool)
		.await
		.unwrap();
	id
}

async fn fund_user(h: &Harness, user: UserId, amount: &str) {
	let tx_ref = TxRef::parse(&format!("fpc-{}", Uuid::new_v4())).unwrap();
	balance_app::record_deposit(&h.deposits, &h.notify, tx_ref, Party::User(user), Network::Bep20, usdt(amount))
		.await
		.unwrap();
	h.relay.drain().await;
}

/// Subscribe and wait for the projection to land — the projection stamps the accrual
/// clocks, so anything that backdates them must run after it.
async fn subscribe(h: &Harness, user: UserId, service: &ServiceId, amount: &str) {
	funds_app::subscribe(&fund_ports(h), &h.subs, user, service.clone(), usdt(amount), now()).await.unwrap();
	h.relay.drain().await;
	for _ in 0..100 {
		if h.accruals.find(user, service).await.unwrap().is_some_and(|accrual| !accrual.cost_basis.is_zero()) {
			return;
		}
		tokio::time::sleep(std::time::Duration::from_millis(50)).await;
	}
	panic!("the subscription's position projection never landed");
}

/// A funded holder of `amount` USDT of the product.
async fn holder(h: &Harness, service: &ServiceId, amount: &str) -> UserId {
	let user = investor(h).await;
	fund_user(h, user, amount).await;
	subscribe(h, user, service, amount).await;
	user
}

async fn backdate(h: &Harness, user: UserId, service: &ServiceId, secs: i64) {
	sqlx::query(
		"UPDATE fund_positions SET fees_accrued_at = now() - make_interval(secs => $3), \
		 crystallized_at = now() - make_interval(secs => $3) WHERE user_id = $1 AND service = $2",
	)
	.bind(user.raw())
	.bind(service.as_str())
	.bind(secs as f64)
	.execute(&h.pool)
	.await
	.unwrap();
}

/// Stand in for the notice period having run: pull a scheduled change's moment into the past.
async fn let_the_notice_run(h: &Harness, change: &FeePolicyChange) {
	sqlx::query("UPDATE fee_policy_changes SET effective_from = now() - interval '1 hour' WHERE id = $1")
		.bind(change.id.raw())
		.execute(&h.pool)
		.await
		.unwrap();
}

/// Clear the global roster and the cooling-off clock, then seat `n` fresh owners.
async fn owners(h: &Harness, n: usize) -> Vec<UserId> {
	sqlx::query("UPDATE users SET role = 'investor' WHERE role = 'owner'").execute(&h.pool).await.unwrap();
	sqlx::query("DELETE FROM governance_roster_change").execute(&h.pool).await.unwrap();
	let mut roster = Vec::with_capacity(n);
	for _ in 0..n {
		let id = investor(h).await;
		sqlx::query("UPDATE users SET role = 'owner' WHERE id = $1").bind(id.raw()).execute(&h.pool).await.unwrap();
		roster.push(id);
	}
	roster
}

/// The token and code mailed to one seat, read out of the queue as the owner would.
async fn credentials(h: &Harness, consilium: ConsiliumId, voter: UserId) -> (String, String) {
	let payload: String = sqlx::query_scalar("SELECT payload::text FROM consilium_mail WHERE consilium_id = $1 AND user_id = $2 AND kind = 'fee_policy_approval'")
		.bind(consilium.raw())
		.bind(voter.raw())
		.fetch_one(&h.pool)
		.await
		.expect("every eligible seat is mailed an approval");
	let mail: serde_json::Value = serde_json::from_str(&payload).unwrap();
	let url = mail["approval_url"].as_str().unwrap().to_owned();
	(url.rsplit('/').next().unwrap().to_owned(), mail["code"].as_str().unwrap().to_owned())
}

async fn vote(h: &Harness, consilium: ConsiliumId, voter: UserId, decision: VoteDecision) -> bool {
	let (token, code) = credentials(h, consilium, voter).await;
	let audit = VoteAudit {
		client_ip: "203.0.113.7".to_owned(),
		user_agent: "itest".to_owned(),
	};
	let outcome = consilium_app::submit_decision(h.consilia.as_ref(), &token, &code, decision, &audit, now()).await.unwrap();
	if outcome.approved {
		consilium_app::execute(&consilium_ports(h), consilium, now()).await.unwrap();
	}
	outcome.decided
}

async fn change_of(h: &Harness, change: &FeePolicyChange) -> FeePolicyChange {
	h.changes.find(change.id).await.unwrap().expect("the change row exists")
}

async fn consilium_state(h: &Harness, id: ConsiliumId) -> ConsiliumState {
	consilium_app::find(h.consilia.as_ref(), id).await.unwrap().consilium.state()
}

/// The queued notices for one change, as `(recipient, payload)`.
async fn notices(h: &Harness, change: &FeePolicyChange) -> Vec<(Uuid, serde_json::Value)> {
	let rows: Vec<(Uuid, String, String)> =
		sqlx::query_as("SELECT user_id, dedupe_key, payload::text FROM consilium_mail WHERE fee_policy_change_id = $1 AND kind = 'fee_policy_notice' ORDER BY user_id")
			.bind(change.id.raw())
			.fetch_all(&h.pool)
			.await
			.unwrap();
	rows.into_iter()
		.map(|(user, key, payload)| {
			assert_eq!(key, format!("fee-policy-notice:{}:{user}", change.id), "one notice per holder, keyed by change and holder");
			(user, serde_json::from_str(&payload).unwrap())
		})
		.collect()
}

async fn fee_debt_of(h: &Harness, user: UserId, service: &ServiceId) -> Usdt {
	h.accruals.find(user, service).await.unwrap().expect("a position").debt
}

fn assert_close(actual: Usdt, expected: Usdt, what: &str) {
	let epsilon = usdt("0.05");
	let diff = if actual > expected {
		actual.checked_sub(expected).unwrap()
	} else {
		expected.checked_sub(actual).unwrap()
	};
	assert!(diff <= epsilon, "{what}: expected ~{expected}, got {actual}");
}

#[tokio::test]
async fn a_change_on_a_fund_with_no_holders_binds_at_once_and_notifies_nobody() {
	let _lock = exclusive().await;
	let Some(h) = harness().await else { return };
	let service = unique_service();
	open_fund(&h, &service).await;

	// From nothing to the house terms: a tightening, but inside the envelope — an
	// administrator's call, and with nobody holding units there is nobody to give notice to.
	let before = now();
	let change = schedule(&h, UserId::new(), &service, FeePolicy::HOUSE, 0, "").await.unwrap();
	assert_eq!(change.requirement, ChangeRequirement::Admin);
	assert_eq!(change.state, FeePolicyChangeState::Scheduled);
	assert!(change.consilium_id.is_none());
	assert!(
		change.effective_from_unix >= before && change.effective_from_unix <= now() + CLOCK_SLACK,
		"binds at once: {}",
		change.effective_from_unix
	);
	assert!(notices(&h, &change).await.is_empty(), "no holders, no notices");
	// Not live until promoted: the policy port still says the product charges nothing.
	assert_eq!(h.policies.find(&service).await.unwrap(), None);
	assert_eq!(h.changes.pending(&service).await.unwrap().map(|pending| pending.id), Some(change.id));

	assert!(fee_app::promote_due(&h.changes, now(), &mut std::collections::HashMap::new()).await.unwrap() >= 1, "the sweeper's promotion picks it up");
	assert_eq!(h.policies.find(&service).await.unwrap(), Some(FeePolicy::HOUSE));
	let current = h.policies.current(&service).await.unwrap().expect("a live row");
	assert_eq!(current.version, 1);
	assert_eq!(current.effective_from_unix, change.effective_from_unix);
	let applied = change_of(&h, &change).await;
	assert_eq!(applied.state, FeePolicyChangeState::Active);
	assert!(applied.applied_at_unix.is_some());
	assert!(h.changes.pending(&service).await.unwrap().is_none());
	// A second promotion of the same change is a no-op, not a second version.
	assert!(!h.changes.promote(change.id, now()).await.unwrap());
}

#[tokio::test]
async fn a_change_over_a_held_fund_waits_out_the_notice_and_mails_every_holder() {
	let _lock = exclusive().await;
	let Some(h) = harness().await else { return };
	let service = unique_service();
	open_fund(&h, &service).await;
	install(&h, &service, FeePolicy::HOUSE).await;
	let first = holder(&h, &service, "1000").await;
	let second = holder(&h, &service, "500").await;

	// A loosening is an administrator's call, but the holders still get their 24 hours.
	let cheaper = policy(100, 2_000, 0, ManagementBasis::InvestedCapital, CrystallizationPeriod::Annual);
	let before = now();
	let change = schedule(&h, UserId::new(), &service, cheaper, 0, "").await.unwrap();
	assert_eq!(change.requirement, ChangeRequirement::Admin);
	assert_eq!(change.state, FeePolicyChangeState::Scheduled);
	assert!(
		change.effective_from_unix >= before + MIN_NOTICE_SECS,
		"never earlier than the notice period: {}",
		change.effective_from_unix
	);
	assert!(change.effective_from_unix <= now() + MIN_NOTICE_SECS + CLOCK_SLACK);

	// One notice per holder, in the same transaction, stating the old and the new terms.
	let queued = notices(&h, &change).await;
	assert_eq!(queued.len(), 2);
	let mut recipients: Vec<Uuid> = queued.iter().map(|(user, _)| *user).collect();
	recipients.sort();
	let mut expected = vec![first.raw(), second.raw()];
	expected.sort();
	assert_eq!(recipients, expected);
	for (_, mail) in &queued {
		assert_eq!(mail["kind"], "fee_policy_notice");
		assert_eq!(mail["fund"], "EV Trading");
		assert_eq!(mail["current"]["management_bps"], 200);
		assert_eq!(mail["proposed"]["management_bps"], 100);
		assert_eq!(mail["effective_at"], change.effective_from_unix);
		assert_eq!(mail["link"], format!("/invest/{service}"));
		assert!(!mail["subject_user_id"].as_str().unwrap().is_empty(), "addressed in the identity plane");
	}

	// Not due yet: the old terms stay live and the sweeper leaves the change alone.
	assert!(!h.changes.due(now()).await.unwrap().contains(&change.id));
	assert!(!h.changes.promote(change.id, now()).await.unwrap());
	assert_eq!(h.policies.find(&service).await.unwrap(), Some(FeePolicy::HOUSE));
	// Once the moment has come, it is promoted and becomes version 2.
	assert!(h.changes.due(change.effective_from_unix + 1).await.unwrap().contains(&change.id));
	assert!(h.changes.promote(change.id, change.effective_from_unix + 1).await.unwrap());
	assert_eq!(h.policies.find(&service).await.unwrap(), Some(cheaper));
	assert_eq!(h.policies.current(&service).await.unwrap().unwrap().version, 2);
	let history = h.changes.list(&service).await.unwrap();
	assert_eq!(
		history.iter().map(|row| (row.version, row.state)).collect::<Vec<_>>(),
		vec![(2, FeePolicyChangeState::Active), (1, FeePolicyChangeState::Superseded)]
	);

	// A later moment the operator asks for is honoured as it is.
	let later = now() + 3 * MIN_NOTICE_SECS;
	let deferred = schedule(&h, UserId::new(), &service, FeePolicy::HOUSE, later, "").await.unwrap();
	assert_eq!(deferred.effective_from_unix, later);
}

#[tokio::test]
async fn one_change_is_on_its_way_per_product_until_it_is_cancelled() {
	let _lock = exclusive().await;
	let Some(h) = harness().await else { return };
	let service = unique_service();
	open_fund(&h, &service).await;
	install(&h, &service, FeePolicy::HOUSE).await;
	holder(&h, &service, "100").await;

	let first = schedule(
		&h,
		UserId::new(),
		&service,
		policy(150, 2_000, 0, ManagementBasis::InvestedCapital, CrystallizationPeriod::Annual),
		0,
		"",
	)
	.await
	.unwrap();
	let err = schedule(
		&h,
		UserId::new(),
		&service,
		policy(100, 2_000, 0, ManagementBasis::InvestedCapital, CrystallizationPeriod::Annual),
		0,
		"",
	)
	.await
	.unwrap_err();
	assert!(matches!(err, DomainError::Conflict(_)), "got {err:?}");
	assert!(err.to_string().contains("cancel it"), "the refusal must say what to do: {err}");

	let admin = UserId::new();
	let cancelled = fee_app::cancel_change(&h.changes, h.consilia.as_ref(), &service, first.id, admin, now()).await.unwrap();
	assert_eq!(cancelled.state, FeePolicyChangeState::Cancelled);
	// Idempotent, and never promoted.
	assert_eq!(
		fee_app::cancel_change(&h.changes, h.consilia.as_ref(), &service, first.id, admin, now()).await.unwrap().state,
		FeePolicyChangeState::Cancelled
	);
	let_the_notice_run(&h, &first).await;
	assert!(!h.changes.promote(first.id, now()).await.unwrap());
	assert_eq!(h.policies.find(&service).await.unwrap(), Some(FeePolicy::HOUSE));
	// The slot is free again, and a change filed under another product's slug is not this
	// product's to cancel.
	let second = schedule(
		&h,
		UserId::new(),
		&service,
		policy(100, 2_000, 0, ManagementBasis::InvestedCapital, CrystallizationPeriod::Annual),
		0,
		"",
	)
	.await
	.unwrap();
	assert!(matches!(
		fee_app::cancel_change(&h.changes, h.consilia.as_ref(), &unique_service(), second.id, admin, now()).await.unwrap_err(),
		DomainError::NotFound { .. }
	));
	// A live version cannot be cancelled: it is not on its way anywhere.
	let active = h.changes.list(&service).await.unwrap().into_iter().find(|row| row.state == FeePolicyChangeState::Active).unwrap();
	assert!(matches!(
		fee_app::cancel_change(&h.changes, h.consilia.as_ref(), &service, active.id, admin, now()).await.unwrap_err(),
		DomainError::Conflict(_)
	));
}

#[tokio::test]
async fn a_tightening_beyond_the_envelope_is_proposed_by_an_owner_and_carried_by_the_owners() {
	let _lock = exclusive().await;
	let Some(h) = harness().await else { return };
	let service = unique_service();
	open_fund(&h, &service).await;
	install(&h, &service, FeePolicy::HOUSE).await;
	let investor = holder(&h, &service, "1000").await;
	let roster = owners(&h, 3).await;

	// An administrator who is not an owner is told exactly why this one is out of reach.
	let err = schedule(&h, UserId::new(), &service, dearer(), 0, "the new mandate costs more to run").await.unwrap_err();
	assert!(matches!(err, DomainError::Forbidden(_)), "got {err:?}");
	assert!(err.to_string().contains("only a fund owner"), "{err}");
	// The owners are never asked to approve a change with no stated reason.
	let err = schedule(&h, roster[0], &service, dearer(), 0, "  ").await.unwrap_err();
	assert!(matches!(err, DomainError::Validation(_)), "got {err:?}");
	assert!(h.changes.pending(&service).await.unwrap().is_none(), "nothing is written before the refusal");

	let opened_at = now();
	let change = schedule(&h, roster[0], &service, dearer(), 0, "the new mandate costs more to run").await.unwrap();
	assert_eq!(change.requirement, ChangeRequirement::OwnerConsilium);
	assert_eq!(change.state, FeePolicyChangeState::AwaitingConsilium);
	assert!(change.scheduled_at_unix.is_none());
	let consilium = change.consilium_id.expect("a change needing the owners names its consilium");
	assert_eq!(consilium_state(&h, consilium).await, ConsiliumState::Open);
	assert!(notices(&h, &change).await.is_empty(), "holders are told when the owners carry it, not before");

	// One approval per eligible seat, naming the fund, both sets of terms and the reason.
	let approvals: Vec<String> = sqlx::query_scalar("SELECT payload::text FROM consilium_mail WHERE consilium_id = $1 AND kind = 'fee_policy_approval'")
		.bind(consilium.raw())
		.fetch_all(&h.pool)
		.await
		.unwrap();
	assert_eq!(approvals.len(), 2, "the initiator holds no seat");
	for payload in &approvals {
		let mail: serde_json::Value = serde_json::from_str(payload).unwrap();
		assert_eq!(mail["fund"], "EV Trading");
		assert_eq!(mail["current"]["management_bps"], 200);
		assert_eq!(mail["proposed"]["management_bps"], 300);
		assert_eq!(mail["reason"], "the new mandate costs more to run");
		assert!(!mail["code"].as_str().unwrap().is_empty());
	}
	// The consilium reads back as its own kind, with the live presentation beside the subject.
	let view = consilium_app::find(h.consilia.as_ref(), consilium).await.unwrap();
	let detail = view.fee_policy.expect("a fee-policy consilium carries its presentation");
	assert_eq!(detail.allocation_name, "EV Trading");
	assert_eq!(detail.holder_count, 1);
	assert_eq!(
		h.consilia.list(10).await.unwrap().first().map(|row| row.consilium.id()),
		Some(consilium),
		"the history rehydrates the new kind"
	);

	// Two of the two peers carry it: the change is scheduled from the moment of the vote,
	// the holder gets notice, and the consilium records the change as its effect.
	assert!(!vote(&h, consilium, roster[1], VoteDecision::Approve).await);
	assert!(vote(&h, consilium, roster[2], VoteDecision::Approve).await);
	assert_eq!(consilium_state(&h, consilium).await, ConsiliumState::Executed);
	let scheduled = change_of(&h, &change).await;
	assert_eq!(scheduled.state, FeePolicyChangeState::Scheduled);
	assert!(scheduled.scheduled_at_unix.is_some_and(|at| at >= opened_at));
	assert!(scheduled.effective_from_unix >= opened_at + MIN_NOTICE_SECS, "the notice clock starts at the carrying vote");
	let queued = notices(&h, &scheduled).await;
	assert_eq!(queued.len(), 1);
	assert_eq!(queued[0].0, investor.raw());
	assert_eq!(queued[0].1["proposed"]["management_bps"], 300);
	let view = consilium_app::find(h.consilia.as_ref(), consilium).await.unwrap();
	assert_eq!(view.consilium.executed_fee_policy_change_id(), Some(change.id));
	assert_eq!(view.consilium.executed_withdrawal_id(), None);
	// A retried execution (the sweeper's) is a no-op, not a second scheduling.
	consilium_app::execute(&consilium_ports(&h), consilium, now()).await.unwrap();
	assert_eq!(notices(&h, &scheduled).await.len(), 1);

	// Still the old terms until the notice has run; then version 2, and the consilium's
	// source claim is free for the next change over this product.
	assert_eq!(h.policies.find(&service).await.unwrap(), Some(FeePolicy::HOUSE));
	let_the_notice_run(&h, &scheduled).await;
	assert!(h.changes.promote(change.id, now()).await.unwrap());
	assert_eq!(h.policies.find(&service).await.unwrap(), Some(dearer()));
	assert_eq!(h.policies.current(&service).await.unwrap().unwrap().version, 2);
	assert_eq!(change_of(&h, &change).await.state, FeePolicyChangeState::Active);
}

#[tokio::test]
async fn a_refused_or_withdrawn_quorum_closes_the_change() {
	let _lock = exclusive().await;
	let Some(h) = harness().await else { return };
	let roster = owners(&h, 3).await;

	// Refused: with three owners a single peer's refusal ends it, and the change goes with it.
	let service = unique_service();
	open_fund(&h, &service).await;
	let change = schedule(&h, roster[0], &service, dearer(), 0, "refused for the record").await.unwrap();
	let consilium = change.consilium_id.unwrap();
	assert!(vote(&h, consilium, roster[1], VoteDecision::Reject).await);
	assert_eq!(consilium_state(&h, consilium).await, ConsiliumState::Rejected);
	assert_eq!(change_of(&h, &change).await.state, FeePolicyChangeState::Rejected);
	assert_eq!(h.policies.find(&service).await.unwrap(), None, "a refused change never reaches the live terms");
	assert!(h.changes.pending(&service).await.unwrap().is_none(), "the slot is free for the next proposal");

	// Withdrawn by another owner: the change is cancelled and its consilium goes with it.
	let change = schedule(&h, roster[0], &service, dearer(), 0, "withdrawn for the record").await.unwrap();
	let consilium = change.consilium_id.unwrap();
	let cancelled = fee_app::cancel_change(&h.changes, h.consilia.as_ref(), &service, change.id, roster[2], now()).await.unwrap();
	assert_eq!(cancelled.state, FeePolicyChangeState::Cancelled);
	assert_eq!(consilium_state(&h, consilium).await, ConsiliumState::Cancelled);
	// A vote arriving after the withdrawal lands on a closed consilium and changes nothing.
	let (token, code) = credentials(&h, consilium, roster[1]).await;
	let audit = VoteAudit {
		client_ip: "203.0.113.7".to_owned(),
		user_agent: "itest".to_owned(),
	};
	assert!(
		consilium_app::submit_decision(h.consilia.as_ref(), &token, &code, VoteDecision::Approve, &audit, now())
			.await
			.is_err()
	);
	assert_eq!(change_of(&h, &change).await.state, FeePolicyChangeState::Cancelled);

	// Withdrawn by the owner who opened it, through the consilium itself: the same cascade,
	// as `rejected` — the change did not get its quorum.
	let change = schedule(&h, roster[0], &service, dearer(), 0, "withdrawn by the initiator").await.unwrap();
	let consilium = change.consilium_id.unwrap();
	consilium_app::cancel(h.consilia.as_ref(), consilium, roster[0], now()).await.unwrap();
	assert_eq!(change_of(&h, &change).await.state, FeePolicyChangeState::Rejected);
}

#[tokio::test]
async fn promotion_settles_the_elapsed_window_at_the_old_rate_before_the_new_one_binds() {
	let _lock = exclusive().await;
	let Some(h) = harness().await else { return };
	let service = unique_service();
	open_fund(&h, &service).await;
	install(&h, &service, FeePolicy::HOUSE).await;
	let investor = holder(&h, &service, "1000").await;
	// A year on the house 2%, never swept.
	backdate(&h, investor, &service, YEAR).await;
	assert_eq!(fee_debt_of(&h, investor, &service).await, Usdt::ZERO);

	// Halve the management rate. The change binds after the notice; stand in for the notice
	// having run.
	let halved = policy(100, 2_000, 0, ManagementBasis::InvestedCapital, CrystallizationPeriod::Annual);
	let change = schedule(&h, UserId::new(), &service, halved, 0, "").await.unwrap();
	let_the_notice_run(&h, &change).await;
	assert!(h.changes.promote(change.id, now()).await.unwrap());
	assert_eq!(h.policies.find(&service).await.unwrap(), Some(halved));

	// The year that had already passed was priced at the OLD 2% — 20 USDT on the 1000
	// invested — and carried into the position's debt, with the clock restarted at the
	// moment the new terms bound. Nobody re-prices time that has passed.
	assert_close(fee_debt_of(&h, investor, &service).await, usdt("20"), "a year at the old rate, carried");
	let accrual = h.accruals.find(investor, &service).await.unwrap().unwrap();
	let effective_from = change_of(&h, &change).await.effective_from_unix;
	assert_eq!(accrual.accrued_at_unix, effective_from, "the elapsed clock restarts where the new terms begin");

	// The next assessment collects the carried 20 plus an hour at the NEW 1% — a hair over
	// 20, nowhere near the 30 a year at 1% plus a re-priced year would be, and nowhere near
	// the 40 of two years at 2%.
	let charge = fee_app::assess_position(
		&h.policies,
		&h.accruals,
		&h.assessments,
		h.ledger.as_ref(),
		&h.nav,
		&h.notify,
		investor,
		service.clone(),
		Trigger::Period,
		now(),
	)
	.await
	.unwrap()
	.expect("the carried debt is collectable");
	h.relay.drain().await;
	assert_close(charge.charge().due, usdt("20"), "carried debt plus an hour at the new rate");
	assert_close(charge.charge().debt_opening, usdt("20"), "the carried year is the opening debt");
	assert_eq!(fee_debt_of(&h, investor, &service).await, Usdt::ZERO, "a 1000-unit holding covers the charge whole");
	let taken = Shares::from_base_units(h.ledger.balance(&LedgerAccountKey::FeeShares(service.clone())).await.unwrap().posted);
	assert_eq!(taken, charge.charge().charged_units);
}

/// A policy written under the OLD schema, above today's ceiling — the very row #233 is
/// about. The ceilings are `NOT VALID`, so an INSERT after the migration is held to them;
/// the only way to stage a legacy row is the way the migration met it: with the constraints
/// off, then re-added `NOT VALID` exactly as `0036` states them.
async fn plant_legacy_policy(h: &Harness, service: &ServiceId, bps: i32) {
	let mut tx = h.pool.begin().await.unwrap();
	for stmt in [
		"ALTER TABLE fee_policies DROP CONSTRAINT fee_policies_management_ceiling",
		"ALTER TABLE fee_policy_changes DROP CONSTRAINT fee_policy_changes_management_ceiling",
	] {
		sqlx::query(stmt).execute(&mut *tx).await.unwrap();
	}
	sqlx::query("INSERT INTO fee_policies (service, management_bps, performance_bps, hurdle_bps, basis, crystallization, updated_by, version, effective_from) VALUES ($1, $2, 2000, 0, 'invested_capital', 'annual', 'legacy', 1, now() - interval '30 days')")
		.bind(service.as_str())
		.bind(bps)
		.execute(&mut *tx)
		.await
		.unwrap();
	sqlx::query(
		"INSERT INTO fee_policy_changes (id, service, version, management_bps, performance_bps, hurdle_bps, basis, crystallization, state, requirement, effective_from, requested_by, requested_at, scheduled_at, applied_at) \
		 VALUES ($1, $2, 1, $3, 2000, 0, 'invested_capital', 'annual', 'active', 'admin', now() - interval '30 days', 'legacy', now() - interval '30 days', now() - interval '30 days', now() - interval '30 days')",
	)
	.bind(Uuid::new_v4())
	.bind(service.as_str())
	.bind(bps)
	.execute(&mut *tx)
	.await
	.unwrap();
	for stmt in [
		"ALTER TABLE fee_policies ADD CONSTRAINT fee_policies_management_ceiling CHECK (management_bps <= 500) NOT VALID",
		"ALTER TABLE fee_policy_changes ADD CONSTRAINT fee_policy_changes_management_ceiling CHECK (state IN ('superseded', 'rejected', 'cancelled') OR management_bps <= 500) NOT VALID",
	] {
		sqlx::query(stmt).execute(&mut *tx).await.unwrap();
	}
	tx.commit().await.unwrap();
}

#[tokio::test]
async fn a_legacy_policy_above_the_ceiling_can_still_be_lowered() {
	let _lock = exclusive().await;
	let Some(h) = harness().await else { return };
	let service = unique_service();
	open_fund(&h, &service).await;
	plant_legacy_policy(&h, &service, 10_000).await;
	let investor = holder(&h, &service, "1000").await;
	// Two hours on the legacy rate, so the promotion has a window to settle at it.
	backdate(&h, investor, &service, 2 * 60 * 60).await;
	assert_eq!(h.policies.find(&service).await.unwrap().map(|p| p.management_bps()), Some(10_000), "the legacy row reads back as it is");

	// Lowering to the house terms is a loosening: one administrator, and the holder's notice.
	let change = schedule(&h, UserId::new(), &service, FeePolicy::HOUSE, 0, "").await.unwrap();
	assert_eq!(change.requirement, ChangeRequirement::Admin);
	assert_eq!(notices(&h, &change).await.len(), 1);
	let_the_notice_run(&h, &change).await;
	// The promotion supersedes the legacy row — an UPDATE the ceiling must not refuse.
	assert!(h.changes.promote(change.id, now()).await.unwrap(), "the over-the-ceiling policy is the one that must always be lowerable");
	assert_eq!(h.policies.find(&service).await.unwrap(), Some(FeePolicy::HOUSE));
	let history = h.changes.list(&service).await.unwrap();
	assert_eq!(history.iter().map(|row| (row.version, row.state, row.policy.management_bps())).collect::<Vec<_>>(), vec![(2, FeePolicyChangeState::Active, 200), (1, FeePolicyChangeState::Superseded, 10_000)]);
	// And the window before the change was priced at the legacy rate, as any other.
	assert!(fee_debt_of(&h, investor, &service).await > Usdt::ZERO);
}

#[tokio::test]
async fn a_requirement_decided_against_stale_terms_is_refused_under_the_lock() {
	let _lock = exclusive().await;
	let Some(h) = harness().await else { return };
	let service = unique_service();
	open_fund(&h, &service).await;
	install(&h, &service, FeePolicy::HOUSE).await;

	// What a request judged before a promotion landed would carry: an administrator's
	// requirement for a change the live terms make the owners'.
	let stale = NewFeePolicyChange {
		id: FeePolicyChangeId::new(),
		service: service.clone(),
		policy: dearer(),
		requirement: ChangeRequirement::Admin,
		requested_effective_from_unix: 0,
		requested_by: UserId::new().to_string(),
		reason: String::new(),
		now_unix: now(),
	};
	let err = h.changes.schedule(&stale, None).await.unwrap_err();
	assert!(matches!(err, DomainError::Conflict(_)), "got {err:?}");
	assert!(err.to_string().contains("re-submit"), "{err}");
	assert!(h.changes.pending(&service).await.unwrap().is_none(), "nothing was recorded");
	// The same terms with the requirement the live terms call for are not stale.
	assert_eq!(fee_app::policy_view(&h.policies, &h.changes, &service).await.unwrap().current.map(|c| c.policy), Some(FeePolicy::HOUSE));
}

#[tokio::test]
async fn a_consilium_gated_change_is_withdrawn_only_by_an_owner() {
	let _lock = exclusive().await;
	let Some(h) = harness().await else { return };
	let roster = owners(&h, 3).await;
	let service = unique_service();
	open_fund(&h, &service).await;
	let change = schedule(&h, roster[0], &service, dearer(), 0, "for the withdrawal test").await.unwrap();
	let consilium = change.consilium_id.unwrap();

	// An administrator who is not an owner could not have opened the quorum and cannot close it.
	let err = fee_app::cancel_change(&h.changes, h.consilia.as_ref(), &service, change.id, UserId::new(), now()).await.unwrap_err();
	assert!(matches!(err, DomainError::Forbidden(_)), "got {err:?}");
	assert_eq!(consilium_state(&h, consilium).await, ConsiliumState::Open);
	assert_eq!(change_of(&h, &change).await.state, FeePolicyChangeState::AwaitingConsilium);
	// Another owner may.
	let cancelled = fee_app::cancel_change(&h.changes, h.consilia.as_ref(), &service, change.id, roster[2], now()).await.unwrap();
	assert_eq!(cancelled.state, FeePolicyChangeState::Cancelled);
	assert_eq!(consilium_state(&h, consilium).await, ConsiliumState::Cancelled);
	// And the proposer, even after leaving the roster: the request was theirs.
	let change = schedule(&h, roster[1], &service, dearer(), 0, "for the proposer test").await.unwrap();
	sqlx::query("UPDATE users SET role = 'investor' WHERE id = $1").bind(roster[1].raw()).execute(&h.pool).await.unwrap();
	assert_eq!(fee_app::cancel_change(&h.changes, h.consilia.as_ref(), &service, change.id, roster[1], now()).await.unwrap().state, FeePolicyChangeState::Cancelled);
	// An administrator's own change stays an administrator's to withdraw.
	let admin_change = schedule(&h, UserId::new(), &service, FeePolicy::HOUSE, 0, "").await.unwrap();
	assert_eq!(fee_app::cancel_change(&h.changes, h.consilia.as_ref(), &service, admin_change.id, UserId::new(), now()).await.unwrap().state, FeePolicyChangeState::Cancelled);
}

#[tokio::test]
async fn a_held_product_gets_no_change_without_a_mailer_and_no_change_beyond_the_horizon() {
	let _lock = exclusive().await;
	let Some(h) = harness().await else { return };
	let service = unique_service();
	open_fund(&h, &service).await;
	let unwired = fee_app::FeePolicyPorts {
		governance_mail_wired: false,
		..policy_ports(&h)
	};
	let request = |policy: FeePolicy, effective_from: i64| fee_app::PolicyChangeRequest {
		service: service.clone(),
		policy,
		requested_effective_from_unix: effective_from,
		reason: String::new(),
	};
	// Nobody holds units: nobody is owed a notice, and the change goes through unwired.
	let first = fee_app::schedule_policy(&unwired, UserId::new(), request(FeePolicy::HOUSE, 0), now()).await.unwrap();
	assert!(h.changes.promote(first.id, now()).await.unwrap());
	holder(&h, &service, "100").await;
	// A holder: the notice is the protection, and a relay that never runs is not notice.
	let err = fee_app::schedule_policy(&unwired, UserId::new(), request(policy(100, 2_000, 0, ManagementBasis::InvestedCapital, CrystallizationPeriod::Annual), 0), now())
		.await
		.unwrap_err();
	assert!(matches!(err, DomainError::Conflict(_)), "got {err:?}");
	assert!(err.to_string().contains("notice"), "{err}");
	assert!(h.changes.pending(&service).await.unwrap().is_none());
	// Wired, the same request is fine — but not a year and a day out.
	let err = schedule(&h, UserId::new(), &service, policy(100, 2_000, 0, ManagementBasis::InvestedCapital, CrystallizationPeriod::Annual), now() + MAX_EFFECTIVE_FROM_HORIZON_SECS + 60, "")
		.await
		.unwrap_err();
	assert!(matches!(err, DomainError::Validation(_)), "got {err:?}");
	assert!(schedule(&h, UserId::new(), &service, policy(100, 2_000, 0, ManagementBasis::InvestedCapital, CrystallizationPeriod::Annual), now() + MAX_EFFECTIVE_FROM_HORIZON_SECS - 60, "").await.is_ok());
}
