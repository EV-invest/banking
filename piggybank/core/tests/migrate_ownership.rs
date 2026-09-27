//! Integration tests for the one-off ownership data migration (issue #245, phase 1,
//! C-6) — real Postgres **and** TigerBeetle (no mocks, per the project rules). They run
//! when `DATABASE_URL` is set and a TigerBeetle replica is reachable (`nix run .#db` +
//! `.#tb`), and skip otherwise.
//!
//! The legacy state is built the way production got there: direct posts onto the retired
//! `fund` (code 1) and `fee` (code 40) claims, a product whose fee class sits on
//! `FeeShares(svc)` with a mark behind it, and — in one variant — a company stake on
//! `CompanyShares(svc)`. What these pin down:
//!
//! 1. **A dry run writes nothing** and the plan says exactly what a run will do.
//! 2. **A run keeps the migration's promise**: the retired claims are empty, both
//!    reserved allocations' supply equals what the named holders were minted, pro rata to
//!    the base unit, at NAV 1.00; the reconciliation is clean; the holders have positions
//!    and can redeem out of the allocation's own cash.
//! 3. **A second run is a no-op**, and a different holder table is refused.
//! 4. **Refusals happen before anything moves**: a table that does not add up, an unknown
//!    or disabled person, a company stake under `keep`.
//!
//! The reserved allocations are one per ledger, so every test that plans or runs the
//! migration shares the same two holders (seated once per binary) and runs serially under
//! the outbox guard; the one test that funds the retired claims is the one that asserts
//! their exact figures.

use std::sync::Arc;

use domain::{
	allocations::{Allocation, AllocationAccess, AllocationIcon, AllocationId},
	auth::AuthSubject,
	balance::{LedgerAccountKey, ServiceId, TransferCode},
	error::DomainError,
	issuance::{IssuanceState, UnitHolder},
	money::{Nav, Network, Shares, Usdt},
	redemptions::RedemptionState,
	users::{Email, UserId},
};
use piggybank_core::{
	application::{
		funds as funds_app,
		issuance::UnitHolding,
		migrate_ownership::{self as migrate_app, CompanyStake, HolderShare, HolderTable, MigrationPlan, MigrationPorts, RunReport, StepStatus},
	},
	infrastructure::{
		allocations::PgAllocations,
		custody::StubCustody,
		issuance::PgUnitIssuances,
		nav::PgNav,
		positions::PgFundPositions,
		reconciliation::{ReconReport, Reconciliation},
		redemptions::PgRedemptions,
		relay::Relay,
		users::PgUsers,
	},
	ports::{
		AllocationRegistry, FundPositionReader, RedemptionRepository, UnitIssuanceRepository, UserRepository,
		ledger::{Ledger, LedgerTransfer},
	},
};
use sqlx::PgPool;
use tokio::sync::{MutexGuard, Notify, OnceCell};
use uuid::Uuid;

mod common;

struct Harness {
	pool: PgPool,
	allocations: Arc<PgAllocations>,
	users: PgUsers,
	nav: PgNav,
	issuances: PgUnitIssuances,
	positions: PgFundPositions,
	reds: PgRedemptions,
	ledger: Arc<dyn Ledger>,
	relay: Relay,
	notify: Arc<Notify>,
	/// Held for the test's whole life — declared last so it is released after the relay.
	_serial: MutexGuard<'static, ()>,
}

async fn harness() -> Option<Harness> {
	let serial = common::outbox_serial().await;
	let pool = common::pool().await?;
	let ledger = common::seeded_ledger(&pool, "ownership migration test").await?;
	let notify = Arc::new(Notify::new());
	Some(Harness {
		allocations: Arc::new(PgAllocations::new(pool.clone())),
		users: PgUsers::new(pool.clone()),
		nav: PgNav::new(pool.clone()),
		issuances: PgUnitIssuances::new(pool.clone()),
		positions: PgFundPositions::new(pool.clone()),
		reds: PgRedemptions::new(pool.clone()),
		relay: Relay::new(pool.clone(), ledger.clone(), Arc::new(StubCustody), notify.clone()),
		ledger,
		notify,
		pool,
		_serial: serial,
	})
}

fn ports(h: &Harness) -> MigrationPorts<'_> {
	MigrationPorts {
		ledger: h.ledger.as_ref(),
		allocations: h.allocations.as_ref(),
		users: &h.users,
		nav: &h.nav,
		issuances: &h.issuances,
	}
}

fn fund_ports(h: &Harness) -> funds_app::FundPorts<'_> {
	funds_app::FundPorts {
		allocations: h.allocations.as_ref(),
		ledger: h.ledger.as_ref(),
		nav: &h.nav,
		relay: &h.notify,
	}
}

fn usdt(decimal: &str) -> Usdt {
	Usdt::parse_decimal(decimal).unwrap()
}

fn shares(decimal: &str) -> Shares {
	Shares::parse_decimal(decimal).unwrap()
}

fn unique_service() -> ServiceId {
	ServiceId::parse(&format!("mig-{}", Uuid::new_v4())).unwrap()
}

fn now_unix() -> i64 {
	std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

async fn cash_of(h: &Harness, key: LedgerAccountKey) -> Usdt {
	Usdt::from_base_units(h.ledger.balance(&key).await.unwrap().posted)
}

async fn units_of(h: &Harness, key: LedgerAccountKey) -> Shares {
	Shares::from_base_units(h.ledger.balance(&key).await.unwrap().posted)
}

/// The retired singletons the migration empties, read through their deprecated keys on
/// purpose: that they go to zero is the assertion.
#[allow(deprecated)]
fn retired_fund() -> LedgerAccountKey {
	LedgerAccountKey::Fund
}

#[allow(deprecated)]
fn retired_fee_revenue() -> LedgerAccountKey {
	LedgerAccountKey::FeeRevenue
}

#[allow(deprecated)]
fn company_shares(service: &ServiceId) -> LedgerAccountKey {
	LedgerAccountKey::CompanyShares(service.clone())
}

/// A real `users` row at KYC tier 1 — a holder must be an active person.
async fn person(h: &Harness) -> UserId {
	let subject = AuthSubject::parse(&format!("mig-{}", Uuid::new_v4())).unwrap();
	let email = Email::parse(&format!("m{}@example.com", Uuid::new_v4().simple())).unwrap();
	let user = h.users.provision(subject, email, true).await.unwrap().id();
	common::set_kyc_level(&h.pool, user, 1).await;
	user
}

/// The two holders every test in this binary names: 80 % / 20 % of both allocations.
/// One pair per binary, because the reserved allocations are one per ledger and the
/// migration's ids are the holders' — a second pair would read as a different table.
async fn holders(h: &Harness) -> (UserId, UserId) {
	static HOLDERS: OnceCell<(UserId, UserId)> = OnceCell::const_new();
	*HOLDERS.get_or_init(|| async { (person(h).await, person(h).await) }).await
}

fn table(fund: &[(UserId, u32)], fee: &[(UserId, u32)]) -> HolderTable {
	let rows = |shares: &[(UserId, u32)]| {
		shares
			.iter()
			.map(|(user_id, share_bps)| HolderShare {
				user_id: *user_id,
				share_bps: *share_bps,
			})
			.collect()
	};
	HolderTable { fund: rows(fund), fee: rows(fee) }
}

fn house_table(a: UserId, b: UserId) -> HolderTable {
	table(&[(a, 8000), (b, 2000)], &[(a, 8000), (b, 2000)])
}

/// A registered, open product admitting every investor.
async fn open_product(h: &Harness, service: &ServiceId) {
	let mut allocation = Allocation::register(AllocationId::new(), service.clone(), "EV Trading", "Systematic crypto", AllocationIcon::default()).unwrap();
	h.allocations.register(&mut allocation).await.unwrap();
	h.allocations.open(service).await.unwrap();
	h.allocations.set_access(service, AllocationAccess::Invest).await.unwrap();
}

/// Legacy cash: custody credited straight to a retired claim, the way the seed and the
/// fee settlements used to land before the ownership model.
async fn credit_legacy(h: &Harness, claim: LedgerAccountKey, amount: &str, code: TransferCode) {
	h.ledger
		.post(&LedgerTransfer {
			id: Uuid::new_v4().as_u128(),
			debit: LedgerAccountKey::CryptoWallet(Network::Bep20),
			credit: claim,
			amount: usdt(amount).base_units(),
			code,
			reference: 0,
		})
		.await
		.expect("credit a retired claim");
}

/// Legacy units on a product: `units` minted into `holder` with no cash, the way an
/// in-kind issuance or a fee charge left them.
async fn mint_legacy(h: &Harness, holder: LedgerAccountKey, service: &ServiceId, units: &str) {
	h.ledger
		.post(&LedgerTransfer {
			id: Uuid::new_v4().as_u128(),
			debit: holder,
			credit: LedgerAccountKey::SharesOutstanding(service.clone()),
			amount: shares(units).base_units(),
			code: TransferCode::UnitIssue,
			reference: 0,
		})
		.await
		.expect("mint legacy units");
}

async fn plan(h: &Harness, table: &HolderTable, company: CompanyStake) -> Result<MigrationPlan, DomainError> {
	migrate_app::plan(&ports(h), table, company, now_unix()).await
}

async fn run(h: &Harness, plan: &MigrationPlan) -> RunReport {
	migrate_app::run(&ports(h), plan).await.expect("the migration runs")
}

async fn scan(h: &Harness) -> ReconReport {
	Reconciliation::new(h.pool.clone(), h.ledger.clone(), h.allocations.clone())
		.scan()
		.await
		.expect("reconciliation scan")
}

fn holding(holders: &[UnitHolding], user: UserId) -> Shares {
	holders.iter().find(|line| line.holder == UnitHolder::User(user)).map(|line| line.units).unwrap_or(Shares::ZERO)
}

/// Clean, and nothing of the reserved allocations' left unheld — the migration's own
/// promise; other products' findings are not this test's.
fn assert_reconciled(report: &ReconReport, when: &str) {
	assert!(report.clean(), "{when}: the reconciliation is not clean: {report:?}");
	assert!(report.units_drift.is_empty(), "{when}: supply drift: {:?}", report.units_drift);
	assert!(
		!report.unheld.iter().any(ServiceId::is_reserved),
		"{when}: a reserved allocation holds value nobody holds: {:?}",
		report.unheld
	);
}

/// The whole path: legacy state → dry run (nothing written) → run (every promise kept,
/// pro rata to the base unit) → second run (no-op) → a different table refused → a
/// holder redeems out of the allocation's own cash at NAV 1.00.
#[tokio::test]
async fn the_migration_seats_the_holders_and_empties_the_retired_claims_once() {
	let Some(h) = harness().await else { return };
	let (a, b) = holders(&h).await;
	let product = unique_service();
	open_product(&h, &product).await;
	// The legacy picture: seed capital on `fund`, retained fees on `fee`, a fee class of
	// 100 units on the product marked at NAV 2.00 (AUM 200 over 100 units), no company stake.
	credit_legacy(&h, retired_fund(), "10000", TransferCode::SeedCapital).await;
	credit_legacy(&h, retired_fee_revenue(), "500", TransferCode::WithdrawFee).await;
	mint_legacy(&h, LedgerAccountKey::FeeShares(product.clone()), &product, "100").await;
	funds_app::post_fund_valuation(h.allocations.as_ref(), &h.nav, h.ledger.as_ref(), product.clone(), usdt("200"), "test", now_unix())
		.await
		.unwrap();
	let fund_before = cash_of(&h, retired_fund()).await;
	let fee_before = cash_of(&h, retired_fee_revenue()).await;
	assert!(
		units_of(&h, LedgerAccountKey::SharesOutstanding(ServiceId::fund())).await.is_zero(),
		"the fund allocation starts empty"
	);
	assert!(
		units_of(&h, LedgerAccountKey::SharesOutstanding(ServiceId::fee())).await.is_zero(),
		"the fee allocation starts empty"
	);

	// ── dry run: the plan, and nothing written ──────────────────────────────────
	let house = house_table(a, b);
	let planned = plan(&h, &house, CompanyStake::Keep).await.unwrap();
	assert_eq!(planned.fund.status, StepStatus::Pending);
	assert_eq!(planned.fund.retired, fund_before);
	assert_eq!(planned.fund.value, fund_before, "the fund's value is its retired claim: no fee classes, an empty service claim");
	assert_eq!(planned.fund.nav, Nav::SEED);
	assert_eq!(planned.fund.grants.len(), 2);
	assert_eq!(planned.fund.grants[0].units, usdt_as_units(fund_before.scale(8000, 10_000).unwrap()), "80 % of the fund");
	assert_eq!(planned.fund.grants[1].units, usdt_as_units(fund_before.scale(2000, 10_000).unwrap()), "20 % of the fund");
	assert_eq!(planned.fee.status, StepStatus::Pending);
	assert_eq!(planned.fee.retired, fee_before);
	let fee_class = planned.fee.holdings.iter().find(|holding| holding.product == product).expect("the product's fee class is priced");
	assert_eq!((fee_class.units, fee_class.nav, fee_class.value), (shares("100"), Nav::parse_decimal("2").unwrap(), usdt("200")));
	let fee_value = fee_before.checked_add(usdt("200")).unwrap().checked_add(planned.fee.claim).unwrap();
	assert_eq!(planned.fee.value, fee_value, "cash on both fee claims plus the fee class at the product's NAV");
	assert_eq!(planned.fee.grants[0].units, usdt_as_units(fee_value.scale(8000, 10_000).unwrap()));
	assert_eq!(
		planned.fee.grants[1].units,
		usdt_as_units(fee_value.checked_sub(fee_value.scale(8000, 10_000).unwrap()).unwrap()),
		"the last holder takes the remainder"
	);
	// Under `keep` a live stake refuses the plan, so what is listed can only be one a
	// sibling test already retired.
	assert!(planned.company.iter().all(|step| step.retired_earlier), "no company stake left anywhere: {:?}", planned.company);
	assert!(!planned.is_noop());
	assert!(planned.fund.grants.iter().chain(&planned.fee.grants).all(|grant| !grant.posted && !grant.row_present));
	// Planning is reading: the retired claims, the supply and the rows are as they were.
	assert_eq!(cash_of(&h, retired_fund()).await, fund_before);
	assert_eq!(cash_of(&h, retired_fee_revenue()).await, fee_before);
	assert!(units_of(&h, LedgerAccountKey::SharesOutstanding(ServiceId::fund())).await.is_zero());
	assert!(units_of(&h, LedgerAccountKey::SharesOutstanding(ServiceId::fee())).await.is_zero());
	for grant in planned.fund.grants.iter().chain(&planned.fee.grants) {
		assert!(h.issuances.find_by_id(grant.issuance_id).await.unwrap().is_none(), "a dry run writes no row");
	}
	let expected_fee_units = [planned.fee.grants[0].units, planned.fee.grants[1].units];
	let expected_fund_units = [planned.fund.grants[0].units, planned.fund.grants[1].units];

	// ── run ─────────────────────────────────────────────────────────────────────
	let report = run(&h, &planned).await;
	assert!(report.fund.chain_posted && report.fee.chain_posted);
	assert_eq!((report.fund.rows_written, report.fee.rows_written), (2, 2));
	assert!(report.company_retired.is_empty());
	assert!(report.after.findings().is_empty(), "the migration's own promise holds: {:?}", report.after.findings());
	assert!(report.after.retired_fund.is_zero() && report.after.retired_fee_revenue.is_zero(), "both retired claims are empty");
	assert_eq!(cash_of(&h, retired_fund()).await, Usdt::ZERO);
	assert_eq!(cash_of(&h, retired_fee_revenue()).await, Usdt::ZERO);

	let fund = &report.after.fund;
	assert_eq!(fund.claim.posted, fund_before, "the seed is now the fund allocation's own cash");
	assert_eq!(fund.units_outstanding, usdt_as_units(fund_before), "one unit per USDT at the seed NAV");
	assert_eq!((holding(&fund.holders, a), holding(&fund.holders, b)), (expected_fund_units[0], expected_fund_units[1]));
	assert!(fund.units_reconcile());
	assert_eq!(report.after.fund_nav, Nav::SEED);

	let fee = &report.after.fee;
	assert_eq!(
		fee.claim.posted,
		fee_before.checked_add(planned.fee.claim).unwrap(),
		"retained fees are now the fee allocation's own cash"
	);
	assert_eq!(fee.units_outstanding, usdt_as_units(fee_value), "units equal the value exactly, remainder included");
	assert_eq!((holding(&fee.holders, a), holding(&fee.holders, b)), (expected_fee_units[0], expected_fee_units[1]));
	assert!(
		fee.product_units.contains(&(product.clone(), shares("100"))),
		"the fee class stays where it was: {:?}",
		fee.product_units
	);
	assert!(fee.units_reconcile());
	assert_eq!(report.after.fee_nav, Nav::SEED, "value / units == 1.00 to the base unit");
	assert_reconciled(&scan(&h).await, "after the run");

	// Rows born applied, and the holders' projections with them: the positions screen
	// lists both allocations for each holder, priced at NAV 1.00.
	for grant in planned.fund.grants.iter().chain(&planned.fee.grants) {
		let record = h.issuances.find_by_id(grant.issuance_id).await.unwrap().expect("the row was written");
		assert_eq!(record.issuance.state(), IssuanceState::Applied);
		assert!(record.applied_at.is_some());
		assert_eq!(record.issuance.units(), grant.units);
		assert_eq!(record.issuance.nav(), Nav::SEED);
		assert_eq!(record.issuance.idempotency_key(), &grant.idempotency_key);
	}
	let position = h.positions.find(a, &ServiceId::fee()).await.unwrap().expect("a's fee position is projected");
	assert_eq!(position.cost_basis, Nav::SEED.value(expected_fee_units[0]).unwrap(), "cost basis = units × 1.00");
	let listed = funds_app::list_positions(&h.positions, h.ledger.as_ref(), &h.nav, b).await.unwrap();
	let fee_view = listed.iter().find(|p| p.service == ServiceId::fee()).expect("b sees fee among their positions");
	assert_eq!((fee_view.units, fee_view.nav), (expected_fee_units[1], Nav::SEED));
	assert!(listed.iter().any(|p| p.service == ServiceId::fund() && p.units == expected_fund_units[1]), "b sees fund too");
	assert_eq!(
		sqlx::query_scalar::<_, i64>("SELECT count(*) FROM outbox WHERE kind = 'issuances' AND aggregate_id = ANY($1)")
			.bind(planned.fund.grants.iter().chain(&planned.fee.grants).map(|g| g.issuance_id.raw()).collect::<Vec<_>>())
			.fetch_one(&h.pool)
			.await
			.unwrap(),
		0,
		"nothing was queued for the relay: the mints were posted here"
	);

	// ── second run: a no-op that says so ────────────────────────────────────────
	let again = plan(&h, &house, CompanyStake::Keep).await.unwrap();
	assert_eq!((again.fund.status, again.fee.status), (StepStatus::AlreadyApplied, StepStatus::AlreadyApplied));
	assert!(again.is_noop());
	assert!(again.fund.grants.iter().chain(&again.fee.grants).all(|grant| grant.posted && grant.row_present));
	assert_eq!(again.fee.grants[1].units, expected_fee_units[1], "an applied grant reports what the ledger minted");
	let report = run(&h, &again).await;
	assert!(!report.fund.chain_posted && !report.fee.chain_posted);
	assert_eq!((report.fund.rows_written, report.fee.rows_written), (0, 0));
	assert_eq!((report.fund.rows_already_present, report.fee.rows_already_present), (2, 2));
	assert_eq!(report.after.fee.units_outstanding, usdt_as_units(fee_value), "nothing minted twice");
	assert_eq!(report.after.fund.units_outstanding, usdt_as_units(fund_before));
	assert_reconciled(&scan(&h).await, "after the second run");

	// A table naming someone else where the ledger already minted to `b`: refused.
	let stranger = person(&h).await;
	let err = plan(&h, &table(&[(a, 8000), (stranger, 2000)], &[(a, 8000), (b, 2000)]), CompanyStake::Keep)
		.await
		.expect_err("a different holder table after the run");
	assert!(matches!(err, DomainError::Conflict(_)), "{err:?}");

	// ── a holder redeems: NAV 1.00, paid out of `service:fee` ─────────────────────
	let claim_before = cash_of(&h, LedgerAccountKey::ServiceClaim(ServiceId::fee())).await;
	let redemption = funds_app::request_redemption(&fund_ports(&h), &h.reds, b, ServiceId::fee(), shares("100"), now_unix())
		.await
		.expect("covered by the allocation's cash, so settled at once");
	common::drain_to_quiescence(&h.relay, &h.pool).await;
	let redemption = h.reds.find_by_id(redemption.id()).await.unwrap().unwrap();
	assert_eq!(redemption.state(), RedemptionState::Completed);
	assert_eq!(redemption.nav(), Some(Nav::SEED));
	assert_eq!(redemption.cash(), Some(usdt("100")));
	assert_eq!(cash_of(&h, LedgerAccountKey::UserClaim(b)).await, usdt("100"), "paid into b's own claim");
	assert_eq!(
		cash_of(&h, LedgerAccountKey::ServiceClaim(ServiceId::fee())).await,
		claim_before.checked_sub(usdt("100")).unwrap()
	);
	assert_eq!(
		units_of(&h, LedgerAccountKey::UserShares(ServiceId::fee(), b)).await,
		expected_fee_units[1].checked_sub(shares("100")).unwrap()
	);
	assert_eq!(
		funds_app::nav_of(&h.nav, h.ledger.as_ref(), &ServiceId::fee()).await.unwrap().nav,
		Nav::SEED,
		"a redemption at NAV leaves NAV"
	);
	assert_reconciled(&scan(&h).await, "after the redemption");
}

/// The same figure as shares: USDT and units share the 18-dp scale, and at the seed NAV
/// one buys exactly one.
fn usdt_as_units(value: Usdt) -> Shares {
	Shares::from_cash(value, Nav::SEED).unwrap()
}

/// Everything that is wrong with the input is refused while the ledger is untouched: a
/// table that does not add up, a person the platform does not know, a disabled one.
#[tokio::test]
async fn a_bad_holder_table_is_refused_before_anything_moves() {
	let Some(h) = harness().await else { return };
	let (a, b) = holders(&h).await;
	let fund_before = cash_of(&h, retired_fund()).await;
	let fee_before = cash_of(&h, retired_fee_revenue()).await;
	let supply_before = units_of(&h, LedgerAccountKey::SharesOutstanding(ServiceId::fund())).await;

	let err = plan(&h, &table(&[(a, 8000), (b, 1000)], &[(a, 10_000)]), CompanyStake::Keep).await.unwrap_err();
	assert!(matches!(err, DomainError::Validation(ref m) if m.contains("9000 bps")), "{err:?}");
	let err = plan(&h, &table(&[(a, 5000), (a, 5000)], &[(b, 10_000)]), CompanyStake::Keep).await.unwrap_err();
	assert!(matches!(err, DomainError::Validation(ref m) if m.contains("twice")), "{err:?}");
	assert!(HolderTable::parse_json(r#"{"fee": [], "fund": []}"#).is_err(), "an empty table");

	let nobody = UserId::new();
	let err = plan(&h, &table(&[(nobody, 10_000)], &[(a, 10_000)]), CompanyStake::Keep).await.unwrap_err();
	assert!(matches!(err, DomainError::NotFound { entity: "user", .. }), "{err:?}");

	let disabled = person(&h).await;
	h.users.disable(disabled).await.unwrap();
	let err = plan(&h, &table(&[(a, 10_000)], &[(disabled, 10_000)]), CompanyStake::Keep).await.unwrap_err();
	assert!(matches!(err, DomainError::Precondition(ref m) if m.contains("not active")), "{err:?}");

	assert_eq!(cash_of(&h, retired_fund()).await, fund_before);
	assert_eq!(cash_of(&h, retired_fee_revenue()).await, fee_before);
	assert_eq!(units_of(&h, LedgerAccountKey::SharesOutstanding(ServiceId::fund())).await, supply_before);
}

/// A product still carrying a company stake blocks the migration under `keep` — the
/// owners' decision, named with the figure — and is burnt under `retire`: supply down by
/// exactly the stake, no cash moved, the reconciliation clean, and a second run finds it
/// retired.
#[tokio::test]
async fn a_company_stake_is_refused_under_keep_and_burnt_under_retire() {
	let Some(h) = harness().await else { return };
	let (a, b) = holders(&h).await;
	let product = unique_service();
	open_product(&h, &product).await;
	mint_legacy(&h, company_shares(&product), &product, "50").await;
	mint_legacy(&h, LedgerAccountKey::UserShares(product.clone(), a), &product, "30").await;
	assert_eq!(units_of(&h, LedgerAccountKey::SharesOutstanding(product.clone())).await, shares("80"));

	let err = plan(&h, &house_table(a, b), CompanyStake::Keep).await.expect_err("keep refuses a non-zero stake");
	assert!(
		matches!(err, DomainError::Precondition(ref m) if m.contains(product.as_str()) && m.contains("50") && m.contains("--company retire")),
		"{err:?}"
	);
	assert_eq!(units_of(&h, company_shares(&product)).await, shares("50"), "refused: nothing burnt");

	let planned = plan(&h, &house_table(a, b), CompanyStake::Retire).await.unwrap();
	let step = planned.company.iter().find(|step| step.product == product).expect("the stake is in the plan");
	assert_eq!(step.units, shares("50"));
	assert!(!step.retired_earlier);
	let report = run(&h, &planned).await;
	assert!(report.company_retired.contains(&product));
	assert_eq!(units_of(&h, company_shares(&product)).await, Shares::ZERO);
	assert_eq!(
		units_of(&h, LedgerAccountKey::SharesOutstanding(product.clone())).await,
		shares("30"),
		"supply shrank by the stake"
	);
	assert_eq!(
		units_of(&h, LedgerAccountKey::UserShares(product.clone(), a)).await,
		shares("30"),
		"the person's units are untouched"
	);
	assert!(report.after.findings().is_empty(), "{:?}", report.after.findings());
	assert_reconciled(&scan(&h).await, "after the stake was retired");

	let again = plan(&h, &house_table(a, b), CompanyStake::Retire).await.unwrap();
	let step = again.company.iter().find(|step| step.product == product).expect("remembered");
	assert!(step.retired_earlier && step.units.is_zero());
	let report = run(&h, &again).await;
	assert!(!report.company_retired.contains(&product), "not burnt twice");
	assert_eq!(units_of(&h, LedgerAccountKey::SharesOutstanding(product.clone())).await, shares("30"));
}
