//! Integration tests for the fee plane — real Postgres **and** TigerBeetle (no mocks,
//! per the project rules). They run when `DATABASE_URL` is set and a TigerBeetle replica
//! is reachable (e.g. `nix run .#db` + `.#tb`), and skip otherwise.
//!
//! What these pin down is the *shape* of the charge, not just its arithmetic (the maths
//! is covered by `domain::fees`' unit tests). Three properties are the whole design and
//! each has a test that would fail loudly if it regressed:
//!
//! 1. **A fee moves units, never cash.** The investor's USDT claim and the fund's claim
//!    are untouched by a charge — which is why collecting one costs no chain fee and can
//!    never drive a balance negative.
//! 2. **Units outstanding do not change.** A charge is a transfer between two holders, so
//!    NAV per unit does not move and no other investor pays for it.
//! 3. **The mark is per investor.** Two holders in the same fund at the same NAV owe
//!    different fees when they entered at different prices.
//!
//! Clocks are moved by backdating the position's accrual columns rather than by waiting,
//! so "a year of holding" is a single SQL statement.

use std::sync::Arc;

use domain::{
	allocations::{Allocation, AllocationId},
	balance::{LedgerAccountKey, Party, ServiceId},
	fees::{CrystallizationPeriod, FeePolicy, ManagementBasis, Trigger},
	money::{Nav, Network, Shares, TxRef, Usdt},
	users::UserId,
};
use piggybank_core::{
	application::{balance as balance_app, fees as fee_app, funds as funds_app},
	infrastructure::{
		allocations::PgAllocations,
		custody::StubCustody,
		db,
		deposits::PgDeposits,
		fee_sweeper::FeeSweeper,
		fees::{PgFeeAssessments, PgFeePolicies, PgFeeSettlements, PgPositionAccruals},
		ledger::{self, TbLedger},
		nav::PgNav,
		redemptions::PgRedemptions,
		relay::Relay,
		subscriptions::PgSubscriptions,
		tigerbeetle::TigerBeetle,
	},
	ports::{
		AllocationRegistry,
		fees::{FeeAssessments, FeePolicies},
		ledger::Ledger,
	},
};
use sqlx::PgPool;
use tokio::sync::Notify;
use uuid::Uuid;

const YEAR: i64 = 365 * 24 * 60 * 60;
/// Backdating by *exactly* one period lands a hair SHORT of the boundary: the accrual
/// clocks are stamped by Postgres `now()` (sub-second) and read back as rounded epoch
/// seconds, while the assessment is handed a `now` from Rust. The gap is under a second
/// either way — irrelevant to a fee (a second of a 2% year is 3e-8 of it, and the next
/// assessment starts from the recorded stamp, so nothing drifts) but fatal to an exact
/// `==`. Tests that need the period to have *closed* therefore overshoot by an hour, and
/// amounts are compared with a tolerance rather than for equality.
const PERIOD_MARGIN: i64 = 60 * 60;
/// Slack for that same sub-second jitter, in USDT. Four orders of magnitude above the
/// jitter and four below the figures being checked.
const EPSILON: &str = "0.05";

struct Harness {
	pool: PgPool,
	allocations: PgAllocations,
	subs: PgSubscriptions,
	reds: PgRedemptions,
	nav: PgNav,
	deposits: PgDeposits,
	policies: PgFeePolicies,
	accruals: PgPositionAccruals,
	assessments: PgFeeAssessments,
	settlements: PgFeeSettlements,
	ledger: Arc<dyn Ledger>,
	relay: Relay,
	notify: Arc<Notify>,
}

async fn harness() -> Option<Harness> {
	let url = std::env::var("DATABASE_URL").ok().filter(|s| !s.is_empty())?;
	let pool = db::connect(&url).await.expect("connect to Postgres");
	db::migrate(&pool).await.expect("apply migrations");

	let address = std::env::var("TIGERBEETLE_ADDRESS").unwrap_or_else(|_| "127.0.0.1:3033".to_owned());
	let cluster = std::env::var("TIGERBEETLE_CLUSTER_ID").ok().and_then(|s| s.parse().ok()).unwrap_or(0u128);
	let tigerbeetle = Arc::new(TigerBeetle::connect(cluster, &address).expect("connect to TigerBeetle"));
	let ledger: Arc<dyn Ledger> = Arc::new(TbLedger::new(tigerbeetle, pool.clone()));
	if ledger::seed_singletons(ledger.as_ref()).await.is_err() {
		eprintln!("TigerBeetle unreachable — skipping fee-policy test");
		return None;
	}

	let notify = Arc::new(Notify::new());
	Some(Harness {
		allocations: PgAllocations::new(pool.clone()),
		subs: PgSubscriptions::new(pool.clone()),
		reds: PgRedemptions::new(pool.clone()),
		nav: PgNav::new(pool.clone()),
		deposits: PgDeposits::new(pool.clone()),
		policies: PgFeePolicies::new(pool.clone()),
		accruals: PgPositionAccruals::new(pool.clone()),
		assessments: PgFeeAssessments::new(pool.clone()),
		settlements: PgFeeSettlements::new(pool.clone()),
		relay: Relay::new(pool.clone(), ledger.clone(), Arc::new(StubCustody), notify.clone()),
		ledger,
		notify,
		pool,
	})
}

fn usdt(decimal: &str) -> Usdt {
	Usdt::parse_decimal(decimal).unwrap()
}

fn shares(decimal: &str) -> Shares {
	Shares::parse_decimal(decimal).unwrap()
}

fn unique_service() -> ServiceId {
	ServiceId::parse(&format!("fee-{}", Uuid::new_v4())).unwrap()
}

fn now_unix() -> i64 {
	std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

/// A registered, open product with the house 2-and-20 terms.
async fn open_fund(h: &Harness, service: &ServiceId) {
	let mut allocation = Allocation::register(AllocationId::new(), service.clone(), "EV Trading", "Systematic crypto").unwrap();
	h.allocations.register(&mut allocation).await.unwrap();
	h.allocations.open(service).await.unwrap();
	h.policies.set(service, FeePolicy::HOUSE, "itest").await.unwrap();
}

async fn fund_user(h: &Harness, user: UserId, amount: &str) {
	let tx_ref = TxRef::parse(&format!("itest-{}", Uuid::new_v4())).unwrap();
	balance_app::record_deposit(&h.deposits, &h.notify, tx_ref, Party::User(user), Network::Bep20, usdt(amount))
		.await
		.unwrap();
	h.relay.drain().await;
}

/// Subscribe and let the relay post both legs plus the position projection.
async fn subscribe(h: &Harness, user: UserId, service: &ServiceId, amount: &str) {
	funds_app::subscribe(&h.allocations, &h.subs, h.ledger.as_ref(), &h.nav, &h.notify, user, service.clone(), usdt(amount), now_unix())
		.await
		.unwrap();
	h.relay.drain().await;
}

/// Move a position's accrual clocks back by `secs` — "hold this for a year" without
/// holding it for a year.
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

async fn assess(h: &Harness, user: UserId, service: &ServiceId) -> Option<domain::fees::FeeCharge> {
	let assessment = fee_app::assess_position(
		&h.policies,
		&h.accruals,
		&h.assessments,
		h.ledger.as_ref(),
		&h.nav,
		&h.notify,
		user,
		service.clone(),
		Trigger::Period,
		now_unix(),
	)
	.await
	.unwrap();
	h.relay.drain().await;
	assessment.map(|a| a.charge())
}

/// Assert two amounts agree to within [`EPSILON`] — see the constant for why an exact
/// comparison is the wrong test here.
fn assert_close(actual: Usdt, expected: Usdt, what: &str) {
	let epsilon = usdt(EPSILON);
	let diff = if actual > expected {
		actual.checked_sub(expected).unwrap()
	} else {
		expected.checked_sub(actual).unwrap()
	};
	assert!(diff <= epsilon, "{what}: expected ~{expected}, got {actual}");
}

async fn units_of(h: &Harness, key: LedgerAccountKey) -> Shares {
	Shares::from_base_units(h.ledger.balance(&key).await.unwrap().posted)
}

async fn cash_of(h: &Harness, key: LedgerAccountKey) -> Usdt {
	Usdt::from_base_units(h.ledger.balance(&key).await.unwrap().posted)
}

#[tokio::test]
async fn a_year_of_holding_costs_two_percent_of_units_and_moves_no_cash_at_all() {
	let Some(h) = harness().await else { return };
	let user = UserId::new();
	let service = unique_service();
	open_fund(&h, &service).await;
	fund_user(&h, user, "1000").await;
	subscribe(&h, user, &service, "1000").await;

	// Balances the fee must not touch, recorded before the charge.
	let claim_before = cash_of(&h, LedgerAccountKey::UserClaim(user)).await;
	let fund_before = cash_of(&h, LedgerAccountKey::ServiceClaim(service.clone())).await;
	let outstanding_before = units_of(&h, LedgerAccountKey::SharesOutstanding(service.clone())).await;
	assert_eq!(outstanding_before, shares("1000"), "1000 USDT at the seed NAV mints 1000 units");
	// `fee` is a singleton account shared by every test in this file, so only its DELTA
	// across this test means anything.
	let revenue_before = cash_of(&h, LedgerAccountKey::FeeRevenue).await;

	backdate(&h, user, &service, YEAR).await;
	let charge = assess(&h, user, &service).await.expect("a year of holding owes a management fee");

	// 2% of the 1000 that actually went in, taken in units at NAV 1.0.
	assert_close(charge.management, usdt("20"), "a year of management on 1000 invested");
	assert_eq!(charge.performance, Usdt::ZERO, "a flat NAV produced no gain, so no performance fee");
	assert_eq!(charge.charged_units, Shares::from_cash(charge.due, Nav::SEED).unwrap(), "the whole charge was collectable");
	assert_eq!(charge.debt_carried, Usdt::ZERO);

	// (1) The units moved from the holder to the manager.
	let taken = charge.charged_units;
	assert_eq!(
		units_of(&h, LedgerAccountKey::UserShares(service.clone(), user)).await,
		shares("1000").checked_sub(taken).unwrap()
	);
	assert_eq!(units_of(&h, LedgerAccountKey::FeeShares(service.clone())).await, taken);

	// (2) Supply is unchanged, so NAV per unit did not move and no other holder paid.
	assert_eq!(units_of(&h, LedgerAccountKey::SharesOutstanding(service.clone())).await, outstanding_before);

	// (3) Not one base unit of cash moved anywhere — the property that makes this free
	// to collect and impossible to overdraw.
	assert_eq!(cash_of(&h, LedgerAccountKey::UserClaim(user)).await, claim_before);
	assert_eq!(cash_of(&h, LedgerAccountKey::ServiceClaim(service.clone())).await, fund_before);
	assert_eq!(
		cash_of(&h, LedgerAccountKey::FeeRevenue).await,
		revenue_before,
		"a charge never credits fee revenue — only a settlement does"
	);
}

#[tokio::test]
async fn a_gain_above_the_mark_adds_the_twenty_percent_and_ratchets_it() {
	let Some(h) = harness().await else { return };
	let user = UserId::new();
	let service = unique_service();
	open_fund(&h, &service).await;
	fund_user(&h, user, "1000").await;
	subscribe(&h, user, &service, "1000").await;

	// The operator marks the fund up 50%: 1000 units are now worth 1500.
	funds_app::post_fund_valuation(&h.allocations, &h.nav, h.ledger.as_ref(), service.clone(), usdt("1500"), "itest", false)
		.await
		.unwrap();

	backdate(&h, user, &service, YEAR + PERIOD_MARGIN).await;
	let charge = assess(&h, user, &service).await.expect("a year at a higher NAV owes both fees");

	// Management is 2% of invested capital; performance is 20% of the gain on what is
	// left after it — not on the pre-fee holding, which would have given a round 100.
	assert_close(charge.management, usdt("20"), "a year of management on 1000 invested");
	assert_close(charge.performance, usdt("98.666666666666666666"), "20% of the gain, net of the management fee");
	assert!(charge.performance < usdt("100"), "performance must not be taken on capital management already claimed");
	assert!(charge.crystallized, "a full year elapsed, so the period closed");
	assert_eq!(charge.high_water_mark, Nav::parse_decimal("1.5").unwrap(), "the mark ratchets to the price it crystallized at");

	// Taken in units at 1.5, and supply is still untouched.
	assert_eq!(charge.charged_units, Shares::from_cash(charge.due, Nav::parse_decimal("1.5").unwrap()).unwrap());
	assert_eq!(units_of(&h, LedgerAccountKey::SharesOutstanding(service.clone())).await, shares("1000"));

	// A second assessment immediately after charges nothing: no time has passed and the
	// mark now equals the NAV, so there is neither management nor gain to charge.
	assert!(assess(&h, user, &service).await.is_none(), "the same gain must never be charged twice");
}

#[tokio::test]
async fn the_mark_is_per_investor_so_a_late_entrant_pays_less() {
	let Some(h) = harness().await else { return };
	let early = UserId::new();
	let late = UserId::new();
	let service = unique_service();
	open_fund(&h, &service).await;

	// The early investor enters at the seed NAV 1.0.
	fund_user(&h, early, "1000").await;
	subscribe(&h, early, &service, "1000").await;

	// The fund doubles, then the late investor enters at 2.0.
	funds_app::post_fund_valuation(&h.allocations, &h.nav, h.ledger.as_ref(), service.clone(), usdt("2000"), "itest", false)
		.await
		.unwrap();
	fund_user(&h, late, "1000").await;
	subscribe(&h, late, &service, "1000").await;

	// Both are assessed a year later at the same price. Only the early investor has a
	// gain above their own mark — the late one is exactly at theirs. A fund-level mark
	// would have charged them the same, which is the inequity this design exists to avoid.
	backdate(&h, early, &service, YEAR + PERIOD_MARGIN).await;
	backdate(&h, late, &service, YEAR + PERIOD_MARGIN).await;
	let early_charge = assess(&h, early, &service).await.expect("the early investor gained");
	let late_charge = assess(&h, late, &service).await.expect("the late investor still owes management");

	assert!(early_charge.performance > Usdt::ZERO, "entered at 1.0, marked at 2.0 — a real gain");
	assert_eq!(late_charge.performance, Usdt::ZERO, "entered at 2.0, still at 2.0 — no gain, no performance fee");
	// Both still pay management on the capital they parked.
	assert_close(early_charge.management, usdt("20"), "the early investor's management fee");
	assert_close(late_charge.management, usdt("20"), "the late investor's management fee");
}

#[tokio::test]
async fn a_recovery_below_the_mark_is_never_charged() {
	let Some(h) = harness().await else { return };
	let user = UserId::new();
	let service = unique_service();
	open_fund(&h, &service).await;
	fund_user(&h, user, "1000").await;
	subscribe(&h, user, &service, "1000").await;

	// Up to 1.4, crystallize there, then down to 0.9 and back up to 1.3. The investor is
	// up 44% over the year just past — and owes nothing on it, because they are still
	// under the mark they already paid at.
	funds_app::post_fund_valuation(&h.allocations, &h.nav, h.ledger.as_ref(), service.clone(), usdt("1400"), "itest", false)
		.await
		.unwrap();
	backdate(&h, user, &service, YEAR + PERIOD_MARGIN).await;
	assess(&h, user, &service).await.expect("the first year crystallizes at 1.4");

	let units_after_first = units_of(&h, LedgerAccountKey::UserShares(service.clone(), user)).await;
	let value_now = Usdt::from_base_units(units_after_first.base_units()).base_units();
	// Mark down to ~0.9 then part-way back to ~1.3, both below the 1.4 mark.
	funds_app::post_fund_valuation(
		&h.allocations,
		&h.nav,
		h.ledger.as_ref(),
		service.clone(),
		Usdt::from_base_units(value_now * 9 / 10),
		"itest",
		false,
	)
	.await
	.unwrap();
	funds_app::post_fund_valuation(
		&h.allocations,
		&h.nav,
		h.ledger.as_ref(),
		service.clone(),
		Usdt::from_base_units(value_now * 13 / 10),
		"itest",
		false,
	)
	.await
	.unwrap();

	backdate(&h, user, &service, YEAR + PERIOD_MARGIN).await;
	let charge = assess(&h, user, &service).await.expect("management still accrues in a drawdown");
	assert_eq!(charge.performance, Usdt::ZERO, "no performance fee on a mere recovery toward the mark");
	assert!(charge.management > Usdt::ZERO, "management is rent on capital and does not care about performance");
}

#[tokio::test]
async fn a_fund_with_no_policy_is_never_charged() {
	let Some(h) = harness().await else { return };
	let user = UserId::new();
	let service = unique_service();
	// Registered and open, but deliberately given no fee policy.
	let mut allocation = Allocation::register(AllocationId::new(), service.clone(), "EV Trading", "Systematic crypto").unwrap();
	h.allocations.register(&mut allocation).await.unwrap();
	h.allocations.open(&service).await.unwrap();

	fund_user(&h, user, "1000").await;
	subscribe(&h, user, &service, "1000").await;
	backdate(&h, user, &service, YEAR * 3).await;

	assert!(assess(&h, user, &service).await.is_none(), "no policy means no fee, however long it is held");
	assert_eq!(units_of(&h, LedgerAccountKey::UserShares(service.clone(), user)).await, shares("1000"));
	assert_eq!(units_of(&h, LedgerAccountKey::FeeShares(service)).await, Shares::ZERO);
}

#[tokio::test]
async fn settling_fee_units_is_the_only_moment_a_fee_becomes_cash() {
	let Some(h) = harness().await else { return };
	let user = UserId::new();
	let service = unique_service();
	open_fund(&h, &service).await;
	fund_user(&h, user, "1000").await;
	subscribe(&h, user, &service, "1000").await;
	backdate(&h, user, &service, YEAR).await;
	assess(&h, user, &service).await.expect("a year owes a fee");

	let fee_units = units_of(&h, LedgerAccountKey::FeeShares(service.clone())).await;
	assert!(fee_units > Shares::ZERO, "the charge accumulated fee units");
	let fund_before = cash_of(&h, LedgerAccountKey::ServiceClaim(service.clone())).await;
	let outstanding_before = units_of(&h, LedgerAccountKey::SharesOutstanding(service.clone())).await;
	// `fee` is a singleton shared with every other test here, so compare the delta.
	let revenue_before = cash_of(&h, LedgerAccountKey::FeeRevenue).await;

	// One bulk conversion for the whole fund — not one per investor. This is what the
	// unit-denominated charge buys: a single ledger operation per period.
	let settlement = fee_app::settle_fee_shares(&h.settlements, h.ledger.as_ref(), &h.nav, &h.notify, service.clone(), None, "itest", now_unix())
		.await
		.unwrap();
	h.relay.drain().await;

	assert_eq!(settlement.units(), fee_units, "omitting `units` settles the whole accumulated balance");
	assert_close(settlement.cash(), usdt("20"), "a year's fee converted at NAV 1.0");
	// Burn-first, pay-second: the units are gone, supply fell by exactly what was burned,
	// and the cash landed in fee revenue out of the fund's claim.
	assert_eq!(units_of(&h, LedgerAccountKey::FeeShares(service.clone())).await, Shares::ZERO);
	assert_eq!(
		units_of(&h, LedgerAccountKey::SharesOutstanding(service.clone())).await,
		outstanding_before.checked_sub(fee_units).unwrap()
	);
	assert_eq!(cash_of(&h, LedgerAccountKey::FeeRevenue).await, revenue_before.checked_add(settlement.cash()).unwrap());
	assert_eq!(cash_of(&h, LedgerAccountKey::ServiceClaim(service)).await, fund_before.checked_sub(settlement.cash()).unwrap());
}

#[tokio::test]
async fn a_settlement_the_fund_cannot_cover_is_refused_not_queued() {
	let Some(h) = harness().await else { return };
	let user = UserId::new();
	let service = unique_service();
	open_fund(&h, &service).await;
	fund_user(&h, user, "1000").await;
	subscribe(&h, user, &service, "1000").await;
	backdate(&h, user, &service, YEAR).await;
	assess(&h, user, &service).await.expect("a year owes a fee");

	let fee_units = units_of(&h, LedgerAccountKey::FeeShares(service.clone())).await;
	let held = units_of(&h, LedgerAccountKey::UserShares(service.clone(), user)).await;

	// The investor redeems everything, which really does drain the fund's claim: the
	// payout settles at NAV 1.0, leaving behind only what the fee units are worth.
	funds_app::request_redemption(&h.allocations, &h.reds, h.ledger.as_ref(), &h.nav, &h.notify, user, service.clone(), held, now_unix())
		.await
		.unwrap();
	h.relay.drain().await;

	// Now mark the remainder up hard. The fee units are suddenly worth several times the
	// cash left in the fund — the one situation a settlement cannot be paid.
	let remaining = cash_of(&h, LedgerAccountKey::ServiceClaim(service.clone())).await;
	funds_app::post_fund_valuation(
		&h.allocations,
		&h.nav,
		h.ledger.as_ref(),
		service.clone(),
		remaining.checked_add(remaining).and_then(|d| d.checked_add(remaining)).unwrap(),
		"itest",
		true,
	)
	.await
	.unwrap();

	// Refused, not queued: nobody is waiting on this, and the fee units keep accumulating
	// at no cost until the fund is liquid again.
	let revenue_before = cash_of(&h, LedgerAccountKey::FeeRevenue).await;
	let err = fee_app::settle_fee_shares(&h.settlements, h.ledger.as_ref(), &h.nav, &h.notify, service.clone(), None, "itest", now_unix())
		.await
		.unwrap_err();
	assert!(matches!(err, domain::error::DomainError::Validation(_)), "got {err:?}");
	// And nothing was destroyed on the way to that refusal.
	assert_eq!(units_of(&h, LedgerAccountKey::FeeShares(service)).await, fee_units);
	assert_eq!(cash_of(&h, LedgerAccountKey::FeeRevenue).await, revenue_before);
}

#[tokio::test]
async fn the_sweeper_charges_every_due_position_and_records_a_statement() {
	let Some(h) = harness().await else { return };
	let user = UserId::new();
	let service = unique_service();
	open_fund(&h, &service).await;
	fund_user(&h, user, "1000").await;
	subscribe(&h, user, &service, "1000").await;
	backdate(&h, user, &service, YEAR).await;

	let sweeper = FeeSweeper::new(
		Arc::new(PgFeePolicies::new(h.pool.clone())),
		Arc::new(PgPositionAccruals::new(h.pool.clone())),
		Arc::new(PgFeeAssessments::new(h.pool.clone())),
		h.ledger.clone(),
		Arc::new(PgNav::new(h.pool.clone())),
		h.notify.clone(),
	);
	assert!(sweeper.sweep(now_unix()).await.unwrap() >= 1, "the backdated position is due");
	h.relay.drain().await;

	// The charge is on the investor's statement, with the figures that explain it.
	let statement = h.assessments.list_by_user(user).await.unwrap();
	let entry = statement.first().expect("one charge");
	assert_eq!(entry.service, service);
	assert_close(entry.management, usdt("20"), "a year of management on 1000 invested");
	assert_eq!(
		units_of(&h, LedgerAccountKey::UserShares(service.clone(), user)).await,
		shares("1000").checked_sub(entry.charged_units).unwrap(),
		"the statement's figure is the one the ledger actually moved"
	);

	// A second sweep in the same second charges nothing — the clocks moved with the charge.
	assert_eq!(sweeper.sweep(now_unix()).await.unwrap(), 0, "a swept position is not due again");
}

#[tokio::test]
async fn a_zero_rate_policy_is_distinct_from_no_policy_and_also_charges_nothing() {
	let Some(h) = harness().await else { return };
	let user = UserId::new();
	let service = unique_service();
	open_fund(&h, &service).await;
	let free = FeePolicy::new(0, 0, 0, ManagementBasis::InvestedCapital, CrystallizationPeriod::Annual).unwrap();
	h.policies.set(&service, free, "itest").await.unwrap();

	fund_user(&h, user, "1000").await;
	subscribe(&h, user, &service, "1000").await;
	backdate(&h, user, &service, YEAR).await;

	assert!(assess(&h, user, &service).await.is_none());
	// The policy is still readable and still says what it says — "charges nothing" is a
	// configured answer, not a missing one.
	assert_eq!(h.policies.find(&service).await.unwrap(), Some(free));
	assert_eq!(units_of(&h, LedgerAccountKey::UserShares(service, user)).await, shares("1000"));
}
