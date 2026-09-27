//! The contract migration `0047_ownership_contract.sql` (#245, C-9) over the history
//! production actually holds — real Postgres and TigerBeetle, no mocks.
//!
//! Production reached 0047 with rows the new vocabulary cannot name: three `deposits` on
//! the retired `piggybank` party (the seed), and on `service_arb` the 13 000 units minted
//! to the company stake (`holder_kind = 'company'`) and handed over to one person
//! (`source = 'company'`), beside a plain mint of 3 250 units to another. The company
//! stake's account `shares_company:service_arb` (code 63) is still in `tb_accounts`, at
//! zero. This suite builds exactly that on a database migrated only to 0046 — through the
//! TigerBeetle client for the legs no live key can post any more, and through the hub's
//! own use cases for everything else — then applies 0047 and pins:
//!
//! 1. **The migration applies over the history** (the narrowing CHECKs on `deposits` and
//!    `unit_issuances` are `NOT VALID`), and **nothing it or the new code touches moves a
//!    holder**: units on the ledger, `fund_positions` (cost basis, high-water mark), the
//!    NAV, each holder's position view, the unit-flow history and the cap table read the
//!    same before and after, and `Σ holders == SharesOutstanding`.
//! 2. **The history reads as history**: the retired issuance rows come back as
//!    `StoredIssuance::RetiredCompany`, never as an error; their keys stay taken; the
//!    retired stake's `tb_accounts` row is stepped over by the cap-table scan; the
//!    seed's deposits still gate their chain references and stay out of every person's
//!    history.
//! 3. **Nothing new can take the retired shapes**: an insert naming `piggybank`,
//!    `company` or `revenue` is refused by a CHECK, and the consilium kinds no longer
//!    admit `revenue_payout`.
//! 4. **The documented rollback round-trips**: the header's rollback statements undo the
//!    CHECKs, and re-applying 0047 over the same rows lands on the same constraints.
//!
//! The suite owns a scratch database, `<DATABASE_URL db>_<binary>_pre0047`, recreated on
//! every run: the binary's usual clone is already migrated past 0046.

use std::sync::Arc;

use domain::{
	allocations::{Allocation, AllocationIcon, AllocationId},
	auth::AuthSubject,
	balance::{AccountCode, LedgerAccountKey, ServiceId, TransferCode},
	error::DomainError,
	issuance::{IdempotencyKey, UnitHolder},
	money::{Nav, Shares, TxRef, Usdt},
	users::{Email, UserId},
};
use piggybank_core::{
	application::{funds as funds_app, issuance as issuance_app},
	infrastructure::{
		allocations::PgAllocations, custody::StubCustody, deposits::PgDeposits, issuance::PgUnitIssuances, nav::PgNav, positions::PgFundPositions, reconciliation::Reconciliation,
		relay::Relay, tigerbeetle::TigerBeetle, users::PgUsers,
	},
	ports::{
		AllocationRegistry, Deposits, FundPositionReader, UnitIssuanceRepository, UserRepository,
		issuance::{RetiredCompanyMovement, StoredIssuance},
		ledger::{HoldingScope, Ledger},
	},
};
use sqlx::{AssertSqlSafe, Connection, PgConnection, PgPool, postgres::PgPoolOptions};
use tigerbeetle as tb;
use tokio::sync::Notify;
use uuid::Uuid;

mod common;

/// The last migration before the contract step.
const EXPAND_VERSION: i64 = 46;

/// The contract step itself. The suite migrates to it and no further: its round trip
/// (rollback, re-apply) is over 0047 alone, and a later migration applied on top would be
/// read back as a constraint 0047 failed to restore.
const CONTRACT_VERSION: i64 = 47;

/// The rollback the 0047 header documents, statement for statement. Kept here so the
/// round trip below exercises exactly what an operator would paste.
const ROLLBACK_0047: &str = "
ALTER TABLE payments DROP CONSTRAINT payments_from_kind_check, DROP CONSTRAINT payments_to_kind_check,
  ADD CONSTRAINT payments_from_kind_check CHECK (from_kind IN ('piggybank', 'user', 'service', 'revenue')),
  ADD CONSTRAINT payments_to_kind_check CHECK (to_kind IN ('piggybank', 'user', 'service', 'revenue'));
ALTER TABLE withdrawals DROP CONSTRAINT withdrawals_source_check,
  ADD CONSTRAINT withdrawals_source_check CHECK (source IN ('user', 'revenue'));
ALTER TABLE consilium DROP CONSTRAINT consilium_kind_check,
  ADD CONSTRAINT consilium_kind_check CHECK (kind IN ('revenue_payout', 'payment', 'valuation_override', 'fee_policy', 'holder_grant', 'seed_capital')),
  ADD CONSTRAINT consilium_payout_spends_the_fee_claim CHECK (kind <> 'revenue_payout' OR source_claim = 'fee');
ALTER TABLE consilium_mail DROP CONSTRAINT consilium_mail_kind_check,
  ADD CONSTRAINT consilium_mail_kind_check CHECK (kind IN ('payout_approval', 'payout_outcome', 'token_burned', 'payment_consent', 'payment_approval', 'fee_policy_approval', 'fee_policy_notice'));
ALTER TABLE deposits DROP CONSTRAINT deposits_party_is_a_user,
  ADD CONSTRAINT deposits_party_is_never_revenue CHECK (party_kind <> 'revenue');
ALTER TABLE unit_issuances DROP CONSTRAINT unit_issuances_holder_kind_check, DROP CONSTRAINT unit_issuances_source_check,
  ADD CONSTRAINT unit_issuances_holder_kind_check CHECK (holder_kind IN ('user', 'company', 'allocation')),
  ADD CONSTRAINT unit_issuances_source_check CHECK (source IN ('mint', 'company', 'retire')),
  ADD CONSTRAINT unit_issuances_company_source_names_a_user CHECK (source <> 'company' OR holder_kind = 'user');
";

const CHECK_VIOLATION: &str = "23514";

fn shares(decimal: &str) -> Shares {
	Shares::parse_decimal(decimal).unwrap()
}

fn usdt(decimal: &str) -> Usdt {
	Usdt::parse_decimal(decimal).unwrap()
}

fn now_unix() -> i64 {
	i64::try_from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()).unwrap()
}

/// Replace the database path segment of a `postgres://…/db[?p]` URL.
fn with_database(url: &str, name: &str) -> String {
	let (base, query) = url.split_once('?').map_or((url, None), |(base, query)| (base, Some(query)));
	let authority_start = base.find("://").map_or(0, |i| i + 3);
	let rebuilt = match base[authority_start..].find('/') {
		Some(slash) => format!("{}/{name}", &base[..authority_start + slash]),
		None => format!("{base}/{name}"),
	};
	query.map_or(rebuilt.clone(), |query| format!("{rebuilt}?{query}"))
}

fn database_of(url: &str) -> String {
	let base = url.split_once('?').map_or(url, |(base, _)| base);
	base.rsplit('/').next().unwrap_or_default().to_owned()
}

/// A fresh database migrated to [`EXPAND_VERSION`] and no further — production's schema on
/// the day before 0047 — or `None` where the suite skips (no `DATABASE_URL` locally).
async fn database_before_contract() -> Option<PgPool> {
	// The binary's own clone: provisioned (and CI-gated) by the shared harness. Its name
	// is valid, lowercase and unique to this binary, so the scratch name derived from it is
	// too, and fits a Postgres identifier.
	let clone_url = common::database_url().await?;
	let scratch = format!("{}_pre0047", database_of(&clone_url));
	assert!(scratch.len() <= 63, "{scratch} does not fit a Postgres identifier");
	let mut admin = PgConnection::connect(&clone_url).await.expect("connect to the binary's database");
	sqlx::query(AssertSqlSafe(format!("DROP DATABASE IF EXISTS {scratch} WITH (FORCE)")))
		.execute(&mut admin)
		.await
		.expect("drop the previous run's scratch database");
	sqlx::query(AssertSqlSafe(format!("CREATE DATABASE {scratch}")))
		.execute(&mut admin)
		.await
		.expect("create the scratch database");
	admin.close().await.expect("close the admin connection");
	let pool = PgPoolOptions::new()
		.max_connections(8)
		.connect(&with_database(&clone_url, &scratch))
		.await
		.expect("connect to the scratch database");
	sqlx::migrate!().run_to(EXPAND_VERSION, &pool).await.expect("migrate the scratch database to 0046");
	Some(pool)
}

struct Harness {
	pool: PgPool,
	ledger: Arc<dyn Ledger>,
	allocations: Arc<PgAllocations>,
	users: PgUsers,
	nav: PgNav,
	issuances: PgUnitIssuances,
	positions: PgFundPositions,
	deposits: PgDeposits,
	relay: Relay,
	notify: Arc<Notify>,
}

impl Harness {
	fn fund_ports(&self) -> funds_app::FundPorts<'_> {
		funds_app::FundPorts {
			allocations: self.allocations.as_ref(),
			ledger: self.ledger.as_ref(),
			nav: &self.nav,
			relay: &self.notify,
		}
	}

	async fn person(&self, name: &str) -> UserId {
		let subject = AuthSubject::parse(&format!("c9-{name}-{}", Uuid::new_v4())).unwrap();
		let email = Email::parse(&format!("{name}-{}@example.test", Uuid::new_v4().simple())).unwrap();
		self.users.provision(subject, email, true).await.unwrap().id()
	}

	/// The TigerBeetle id `tb_accounts` holds for a logical key.
	async fn account_id(&self, logical_key: &str) -> u128 {
		let bytes: Vec<u8> = sqlx::query_scalar("SELECT tb_account_id FROM tb_accounts WHERE logical_key = $1")
			.bind(logical_key)
			.fetch_one(&self.pool)
			.await
			.unwrap_or_else(|err| panic!("{logical_key} is mapped: {err}"));
		u128::from_be_bytes(bytes.try_into().unwrap())
	}
}

/// A raw TigerBeetle client on the same replica the ledger uses — for the two legs of the
/// company stake, which no live key can post any more.
fn raw_tigerbeetle() -> TigerBeetle {
	let address = std::env::var("TIGERBEETLE_ADDRESS").unwrap_or_else(|_| "127.0.0.1:3033".to_owned());
	let cluster = std::env::var("TIGERBEETLE_CLUSTER_ID").ok().and_then(|s| s.parse().ok()).unwrap_or(0u128);
	TigerBeetle::connect(cluster, &address).expect("connect to TigerBeetle")
}

async fn post_raw(tb: &TigerBeetle, debit: u128, credit: u128, units: Shares, code: u16) {
	let transfer = tb::Transfer {
		id: Uuid::new_v4().as_u128(),
		debit_account_id: debit,
		credit_account_id: credit,
		amount: units.base_units(),
		ledger: domain::balance::Ledger::Share.id(),
		code,
		..Default::default()
	};
	let results = tb.client().create_transfers(&[transfer]).expect("tigerbeetle open").await.expect("create_transfers");
	for result in results {
		assert!(matches!(result.status, tb::CreateTransferStatus::Created), "the historical leg posts: {:?}", result.status);
	}
}

/// What production's `service_arb` looked like the day 0047 shipped, built on `h`.
struct History {
	service: ServiceId,
	/// Holds 13 000 units, handed to them out of the company stake.
	valera: UserId,
	/// Holds 3 250 units, minted to them directly.
	inin: UserId,
	seed_deposits: Vec<TxRef>,
}

async fn build_history(h: &Harness) -> History {
	let service = ServiceId::parse("service_arb").unwrap();
	let mut allocation = Allocation::register(AllocationId::new(), service.clone(), "Service Arb", "Arbitrage desk", AllocationIcon::default()).unwrap();
	h.allocations.register(&mut allocation).await.unwrap();
	h.allocations.open(&service).await.unwrap();
	let valera = h.person("valera").await;
	let inin = h.person("inin").await;

	// The company stake, as v0.x wrote it: `shares_company:service_arb` (code 63,
	// debit-normal) minted 13 000 against the supply, then handed to a person — two legs
	// under the transfer codes of the day (47 in-kind mint, 52 stake hand-over).
	let tb = raw_tigerbeetle();
	let company_key = format!("shares_company:{service}");
	let company_id = Uuid::new_v4().as_u128();
	let company = tb::Account {
		id: company_id,
		ledger: domain::balance::Ledger::Share.id(),
		code: AccountCode::RetiredCompanyStake.code(),
		flags: tb::AccountFlags::CreditsMustNotExceedDebits,
		..Default::default()
	};
	let created = tb.client().create_accounts(&[company]).expect("tigerbeetle open").await.expect("create_accounts");
	assert!(created.iter().all(|r| matches!(r.status, tb::CreateAccountStatus::Created)), "{created:?}");
	sqlx::query("INSERT INTO tb_accounts (logical_key, tb_account_id, ledger, code, network, flags) VALUES ($1, $2, $3, $4, NULL, $5)")
		.bind(&company_key)
		.bind(&company_id.to_be_bytes()[..])
		.bind(i32::try_from(domain::balance::Ledger::Share.id()).unwrap())
		.bind(i32::from(AccountCode::RetiredCompanyStake.code()))
		.bind(i32::from(tb::AccountFlags::CreditsMustNotExceedDebits.bits()))
		.execute(&h.pool)
		.await
		.unwrap();
	let outstanding = LedgerAccountKey::SharesOutstanding(service.clone());
	let valera_shares = LedgerAccountKey::UserShares(service.clone(), valera);
	h.ledger.ensure_account(&outstanding).await.unwrap();
	h.ledger.ensure_account(&valera_shares).await.unwrap();
	let stake = shares("13000");
	post_raw(&tb, company_id, h.account_id(&outstanding.logical_key()).await, stake, TransferCode::UnitIssue.code()).await;
	post_raw(&tb, h.account_id(&valera_shares.logical_key()).await, company_id, stake, 52).await;
	sqlx::query(
		"INSERT INTO unit_issuances (id, service, holder_kind, holder_id, source, units, nav, cost_basis, idempotency_key, state, applied_at) VALUES \
		 ($1, $3, 'company', NULL, 'mint', $5, $6, $5, 'company-mint', 'applied', now() - interval '2 days'), \
		 ($2, $3, 'user', $4, 'company', $5, $6, $5, 'company-transfer', 'applied', now() - interval '1 day')",
	)
	.bind(Uuid::new_v4())
	.bind(Uuid::new_v4())
	.bind(service.as_str())
	.bind(valera.raw())
	.bind(stake.base_units().to_string())
	.bind(Nav::SEED.base_units().to_string())
	.execute(&h.pool)
	.await
	.unwrap();
	// The projection the relay wrote for the hand-over: the stated basis at the seed NAV.
	sqlx::query("INSERT INTO fund_positions (user_id, service, cost_basis, units, high_water_mark) VALUES ($1, $2, $3, $3, $4)")
		.bind(valera.raw())
		.bind(service.as_str())
		.bind(stake.base_units().to_string())
		.bind(Nav::SEED.base_units().to_string())
		.execute(&h.pool)
		.await
		.unwrap();

	// The other holder came in through the ordinary door, on this very build.
	issuance_app::issue_units(
		&h.fund_ports(),
		&h.issuances,
		&h.users,
		issuance_app::IssueUnitsRequest {
			service: service.clone(),
			holder: UnitHolder::User(inin),
			units: shares("3250"),
			cost_basis: Some(usdt("3250")),
			idempotency_key: IdempotencyKey::parse("inin").unwrap(),
		},
		now_unix(),
	)
	.await
	.unwrap();
	common::drain_to_quiescence(&h.relay, &h.pool).await;

	// The mark production carries: NAV 1.00 over 16 250 units.
	funds_app::post_fund_valuation(h.allocations.as_ref(), &h.nav, h.ledger.as_ref(), service.clone(), usdt("16250"), "c9", now_unix())
		.await
		.unwrap();

	// The seed's three deposits onto the retired fund party.
	let mut seed_deposits = Vec::new();
	for amount in ["1000", "2500", "40"] {
		let tx_ref = TxRef::parse(&format!("0xseed{}", Uuid::new_v4().simple())).unwrap();
		sqlx::query("INSERT INTO deposits (tx_ref, party_kind, party_id, network, amount, event_id) VALUES ($1, 'piggybank', NULL, 'bep20', $2, $3)")
			.bind(tx_ref.as_str())
			.bind(usdt(amount).base_units().to_string())
			.bind(Uuid::new_v4())
			.execute(&h.pool)
			.await
			.unwrap();
		seed_deposits.push(tx_ref);
	}
	History {
		service,
		valera,
		inin,
		seed_deposits,
	}
}

/// Everything a holder or the operator reads about `service_arb`, in one comparable value.
#[derive(Debug, PartialEq)]
struct Picture {
	units_outstanding: Shares,
	cap_table: Vec<issuance_app::UnitHolding>,
	nav: Nav,
	valera_units: u128,
	inin_units: u128,
	/// `(cost_basis, units, high_water_mark)` as stored, per holder.
	valera_row: (String, String, String),
	inin_row: (String, String, String),
	/// `(units, nav, value, cost_basis)` of each holder's position card.
	valera_card: (Shares, Nav, Usdt, Usdt),
	inin_card: (Shares, Nav, Usdt, Usdt),
	/// Σ of each holder's unit-flow history (the position chart's series).
	valera_flow: i128,
	inin_flow: i128,
	units_drift: bool,
}

async fn picture(h: &Harness, history: &History) -> Picture {
	let service = &history.service;
	let view = issuance_app::unit_holders(h.allocations.as_ref(), h.ledger.as_ref(), &h.issuances, service.clone())
		.await
		.unwrap();
	let row = |user: UserId| async move {
		sqlx::query_as::<_, (String, String, String)>("SELECT cost_basis, units, high_water_mark FROM fund_positions WHERE user_id = $1 AND service = $2")
			.bind(user.raw())
			.bind(service.as_str())
			.fetch_one(&h.pool)
			.await
			.unwrap()
	};
	let card = |user: UserId| async move {
		let card = funds_app::get_position(&h.positions, h.ledger.as_ref(), &h.nav, user, service.clone()).await.unwrap();
		(card.units, card.nav, card.value, card.cost_basis)
	};
	let flow = |user: UserId| async move { h.positions.unit_flows(user, service).await.unwrap().iter().map(|f| f.delta).sum::<i128>() };
	let units = |user: UserId| async move { h.ledger.balance(&LedgerAccountKey::UserShares(service.clone(), user)).await.unwrap().posted };
	let recon = Reconciliation::new(h.pool.clone(), h.ledger.clone(), h.allocations.clone() as Arc<dyn AllocationRegistry>)
		.scan()
		.await
		.unwrap();
	Picture {
		units_outstanding: view.units_outstanding,
		cap_table: view.holders,
		nav: funds_app::nav_of(&h.nav, h.ledger.as_ref(), service).await.unwrap().nav,
		valera_units: units(history.valera).await,
		inin_units: units(history.inin).await,
		valera_row: row(history.valera).await,
		inin_row: row(history.inin).await,
		valera_card: card(history.valera).await,
		inin_card: card(history.inin).await,
		valera_flow: flow(history.valera).await,
		inin_flow: flow(history.inin).await,
		units_drift: recon.units_drift.contains(service),
	}
}

/// The CHECK constraints 0047 owns, as the catalog spells them — what the rollback round
/// trip must land back on.
async fn contract_checks(pool: &PgPool) -> Vec<(String, String, bool)> {
	sqlx::query_as(
		"SELECT conname::text, pg_get_constraintdef(oid), convalidated FROM pg_constraint \
		 WHERE conrelid IN ('payments'::regclass, 'withdrawals'::regclass, 'consilium'::regclass, 'consilium_mail'::regclass, 'deposits'::regclass, 'unit_issuances'::regclass) \
		 AND contype = 'c' ORDER BY conrelid::regclass::text, conname",
	)
	.fetch_all(pool)
	.await
	.unwrap()
}

async fn sqlstate_of(result: Result<sqlx::postgres::PgQueryResult, sqlx::Error>) -> Option<String> {
	match result {
		Ok(_) => None,
		Err(sqlx::Error::Database(err)) => err.code().map(|code| code.into_owned()),
		Err(other) => panic!("not a database refusal: {other}"),
	}
}

#[tokio::test]
async fn the_contract_migration_applies_over_production_history_and_moves_no_holder() {
	let _serial = common::outbox_serial().await;
	let Some(pool) = database_before_contract().await else { return };
	let Some(ledger) = common::seeded_ledger(&pool, "ownership contract test").await else {
		return;
	};
	let notify = Arc::new(Notify::new());
	let h = Harness {
		allocations: Arc::new(PgAllocations::new(pool.clone())),
		users: PgUsers::new(pool.clone()),
		nav: PgNav::new(pool.clone()),
		issuances: PgUnitIssuances::new(pool.clone()),
		positions: PgFundPositions::new(pool.clone()),
		deposits: PgDeposits::new(pool.clone()),
		relay: Relay::new(pool.clone(), ledger.clone(), Arc::new(StubCustody), notify.clone()),
		ledger,
		notify,
		pool: pool.clone(),
	};
	let history = build_history(&h).await;
	let before = picture(&h, &history).await;

	// (1) The migration applies over the history, and the holders are exactly as they were.
	sqlx::migrate!().run_to(CONTRACT_VERSION, &pool).await.expect("0047 applies over production's history");
	let applied: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM _sqlx_migrations WHERE version = 47 AND success)")
		.fetch_one(&pool)
		.await
		.unwrap();
	assert!(applied, "0047 is recorded as applied");
	let after = picture(&h, &history).await;
	assert_eq!(after, before, "the contract step moves no holder");

	// ...and what they are is production's `service_arb`, in absolute terms.
	let stake = shares("13000").base_units();
	let minted = shares("3250").base_units();
	assert_eq!(after.units_outstanding, shares("16250"));
	assert_eq!((after.valera_units, after.inin_units), (stake, minted));
	assert_eq!(
		after.cap_table,
		vec![
			issuance_app::UnitHolding {
				holder: UnitHolder::User(history.valera),
				units: shares("13000")
			},
			issuance_app::UnitHolding {
				holder: UnitHolder::User(history.inin),
				units: shares("3250")
			},
		],
		"people only — the retired company stake is not a line"
	);
	let held = after.cap_table.iter().fold(Shares::ZERO, |sum, line| sum.checked_add(line.units).unwrap());
	assert_eq!(held, after.units_outstanding, "Σ holders == SharesOutstanding");
	assert!(!after.units_drift, "units_reconcile is clean on service_arb");
	assert_eq!(after.nav, Nav::SEED, "the 1.00 mark");
	assert_eq!(after.valera_card, (shares("13000"), Nav::SEED, usdt("13000"), usdt("13000")));
	assert_eq!(after.inin_card, (shares("3250"), Nav::SEED, usdt("3250"), usdt("3250")));
	assert_eq!(after.valera_row.2, Nav::SEED.base_units().to_string(), "the high-water mark is untouched");
	assert_eq!(after.valera_flow, i128::try_from(stake).unwrap(), "the hand-over is still in the holder's unit history");

	// (2) The history reads as history. The retired stake's map row is stepped over, not
	// parsed (it still holds zero on the ledger).
	let scan = h
		.ledger
		.share_holdings(&HoldingScope::Product(history.service.clone()))
		.await
		.expect("the cap-table scan steps over the retired row");
	assert!(scan.iter().all(|(key, _)| !key.logical_key().starts_with("shares_company:")), "{scan:?}");
	assert_eq!(scan.len(), 2, "two holders, nothing else: {scan:?}");
	let key = |raw: &str| IdempotencyKey::parse(raw).unwrap();
	let Some(StoredIssuance::RetiredCompany(minted_to_company)) = h.issuances.find_by_key(&history.service, &key("company-mint")).await.unwrap() else {
		panic!("the company mint reads as history")
	};
	assert_eq!(minted_to_company.movement, RetiredCompanyMovement::MintedToCompany);
	assert_eq!(minted_to_company.units, shares("13000"));
	assert!(minted_to_company.applied_at.is_some());
	let Some(StoredIssuance::RetiredCompany(handed)) = h.issuances.find_by_key(&history.service, &key("company-transfer")).await.unwrap() else {
		panic!("the hand-over reads as history")
	};
	assert_eq!(handed.movement, RetiredCompanyMovement::HandedTo(history.valera));
	let by_id = h.issuances.find_by_id(handed.id).await.unwrap().expect("found by id too");
	assert!(matches!(by_id, StoredIssuance::RetiredCompany(ref row) if row == &handed));
	// Its key is taken for good: a new request under it is a conflict, not a mint.
	let reuse = issuance_app::issue_units(
		&h.fund_ports(),
		&h.issuances,
		&h.users,
		issuance_app::IssueUnitsRequest {
			service: history.service.clone(),
			holder: UnitHolder::User(history.valera),
			units: shares("1"),
			cost_basis: None,
			idempotency_key: key("company-transfer"),
		},
		now_unix(),
	)
	.await
	.unwrap_err();
	assert!(matches!(reuse, DomainError::Conflict(ref why) if why.contains("historical")), "got {reuse:?}");
	// The ordinary row next to them is still live.
	assert!(matches!(h.issuances.find_by_key(&history.service, &key("inin")).await.unwrap(), Some(StoredIssuance::Live(_))));
	// The seed's deposits still gate their chain references and stay out of every person's
	// history.
	for tx_ref in &history.seed_deposits {
		assert!(h.deposits.is_recorded(tx_ref).await.unwrap(), "{tx_ref:?} is still a spent reference");
	}
	for person in [history.valera, history.inin] {
		assert!(h.deposits.list_by_user(person).await.unwrap().is_empty(), "the seed is nobody's deposit history");
	}

	// (3) Nothing new can take a retired shape.
	let refused = [
		sqlx::query("INSERT INTO deposits (tx_ref, party_kind, party_id, network, amount, event_id) VALUES ($1, 'piggybank', NULL, 'bep20', '1', $2)")
			.bind(format!("0xnew{}", Uuid::new_v4().simple()))
			.bind(Uuid::new_v4())
			.execute(&pool)
			.await,
		sqlx::query("INSERT INTO deposits (tx_ref, party_kind, party_id, network, amount, event_id) VALUES ($1, 'service', 'fee', 'bep20', '1', $2)")
			.bind(format!("0xnew{}", Uuid::new_v4().simple()))
			.bind(Uuid::new_v4())
			.execute(&pool)
			.await,
		sqlx::query(
			"INSERT INTO unit_issuances (id, service, holder_kind, holder_id, source, units, nav, cost_basis, idempotency_key, state) \
			 VALUES ($1, $2, 'company', NULL, 'mint', '1', '1', '0', 'new-company-mint', 'queued')",
		)
		.bind(Uuid::new_v4())
		.bind(history.service.as_str())
		.execute(&pool)
		.await,
		sqlx::query(
			"INSERT INTO unit_issuances (id, service, holder_kind, holder_id, source, units, nav, cost_basis, idempotency_key, state) \
			 VALUES ($1, $2, 'user', $3, 'company', '1', '1', '0', 'new-hand-over', 'queued')",
		)
		.bind(Uuid::new_v4())
		.bind(history.service.as_str())
		.bind(history.inin.raw())
		.execute(&pool)
		.await,
		sqlx::query("INSERT INTO withdrawals (id, source, user_id, network, address, amount, fee, state) VALUES ($1, 'revenue', NULL, 'bep20', '0x0', '1', '0', 'queued')")
			.bind(Uuid::new_v4())
			.execute(&pool)
			.await,
	];
	for (i, result) in refused.into_iter().enumerate() {
		assert_eq!(
			sqlstate_of(result).await.as_deref(),
			Some(CHECK_VIOLATION),
			"insert #{i} of a retired shape must be refused by a CHECK"
		);
	}
	let checks = contract_checks(&pool).await;
	let def = |name: &str| checks.iter().find(|(n, ..)| n == name).unwrap_or_else(|| panic!("{name} exists: {checks:?}")).1.clone();
	for (name, retired) in [
		("payments_from_kind_check", "piggybank"),
		("payments_to_kind_check", "revenue"),
		("consilium_kind_check", "revenue_payout"),
		("consilium_mail_kind_check", "payout_approval"),
	] {
		assert!(!def(name).contains(retired), "{name} still admits {retired}: {}", def(name));
	}
	assert!(checks.iter().all(|(name, ..)| name != "consilium_payout_spends_the_fee_claim"), "the payout-only CHECK is gone");
	for name in ["deposits_party_is_a_user", "unit_issuances_holder_kind_check", "unit_issuances_source_check"] {
		let validated = checks.iter().find(|(n, ..)| n == name).unwrap().2;
		assert!(!validated, "{name} is NOT VALID: the history it would fail stays as written");
	}

	// (4) The documented rollback, then 0047 again, over the same rows.
	sqlx::raw_sql(ROLLBACK_0047).execute(&pool).await.expect("the documented rollback applies");
	let rolled_back = contract_checks(&pool).await;
	assert_ne!(rolled_back, checks, "the rollback changed the constraints");
	sqlx::raw_sql(include_str!("../migrations/0047_ownership_contract.sql"))
		.execute(&pool)
		.await
		.expect("0047 re-applies after its rollback");
	assert_eq!(contract_checks(&pool).await, checks, "apply → roll back → apply lands on the same constraints");
	assert_eq!(picture(&h, &history).await, before, "and still moves no holder");
}
