//! Integration tests for the allocation registry — real Postgres **and** TigerBeetle
//! (no mocks, per the project rules). They run when `DATABASE_URL` is set and a
//! TigerBeetle replica is reachable (e.g. `nix run .#db` + `.#tb`), and skip otherwise.
//!
//! The load-bearing behaviour here is the **gate**: before this registry existed,
//! `subscribe` accepted any well-formed slug and a service with no valuation bootstrapped
//! silently at the seed NAV — so a user could mint a fund by typing an unknown id. These
//! tests pin that door shut, and pin the one door that must stay open: a `closed`
//! allocation still redeems, so winding a product down never traps an investor.

use std::sync::Arc;

use domain::{
	allocations::{Allocation, AllocationIcon, AllocationId, AllocationState},
	balance::{LedgerAccountKey, Party, ServiceId},
	money::{Network, Shares, TxRef, Usdt},
	users::UserId,
};
use piggybank_core::{
	application::{balance as balance_app, funds as funds_app},
	infrastructure::{
		allocations::PgAllocations,
		custody::StubCustody,
		db,
		deposits::PgDeposits,
		ledger::{self, TbLedger},
		nav::PgNav,
		redemptions::PgRedemptions,
		relay::Relay,
		subscriptions::PgSubscriptions,
		tigerbeetle::TigerBeetle,
	},
	ports::{AllocationRegistry, ledger::Ledger},
};
use sqlx::{
	AssertSqlSafe, PgPool,
	postgres::{PgConnectOptions, PgPoolOptions},
};
use tokio::sync::Notify;
use uuid::Uuid;

struct Harness {
	pool: PgPool,
	allocations: PgAllocations,
	subs: PgSubscriptions,
	reds: PgRedemptions,
	nav: PgNav,
	deposits: PgDeposits,
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
		eprintln!("TigerBeetle unreachable — skipping allocation-registry test");
		return None;
	}

	let notify = Arc::new(Notify::new());
	Some(Harness {
		allocations: PgAllocations::new(pool.clone()),
		subs: PgSubscriptions::new(pool.clone()),
		reds: PgRedemptions::new(pool.clone()),
		nav: PgNav::new(pool.clone()),
		deposits: PgDeposits::new(pool.clone()),
		relay: Relay::new(pool.clone(), ledger.clone(), Arc::new(StubCustody), notify.clone()),
		ledger,
		notify,
		pool,
	})
}

/// The dealing ports, borrowed out of the harness for one call — the same wiring behind
/// every fund use-case in this suite.
fn fund_ports(h: &Harness) -> funds_app::FundPorts<'_> {
	funds_app::FundPorts {
		allocations: &h.allocations,
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
	ServiceId::parse(&format!("svc-{}", Uuid::new_v4())).unwrap()
}

fn now_unix() -> i64 {
	std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

async fn register(h: &Harness, service: &ServiceId) -> Allocation {
	register_with_icon(h, service, AllocationIcon::default()).await
}

async fn register_with_icon(h: &Harness, service: &ServiceId, icon: AllocationIcon) -> Allocation {
	let mut allocation = Allocation::register(AllocationId::new(), service.clone(), "EV Trading", "Systematic crypto", icon).unwrap();
	h.allocations.register(&mut allocation).await.unwrap();
	allocation
}

/// Credit `amount` to the user's unified claim so a subscribe has money to move.
async fn fund_user(h: &Harness, user: UserId, amount: &str) {
	let tx_ref = TxRef::parse(&format!("itest-{}", Uuid::new_v4())).unwrap();
	balance_app::record_deposit(&h.deposits, &h.notify, tx_ref, Party::User(user), Network::Bep20, usdt(amount))
		.await
		.unwrap();
	h.relay.drain().await;
}

async fn subscribe(h: &Harness, user: UserId, service: &ServiceId, amount: &str) -> Result<(), domain::error::DomainError> {
	funds_app::subscribe(&fund_ports(h), &h.subs, user, service.clone(), usdt(amount), now_unix()).await.map(|_| ())
}

#[tokio::test]
async fn an_unregistered_service_cannot_be_subscribed_into() {
	let Some(h) = harness().await else { return };
	let user = UserId::new();
	let service = unique_service();
	fund_user(&h, user, "100").await;

	// The regression this whole feature exists for: a well-formed but unknown slug used
	// to mint a fund at the seed NAV. It must now be refused outright.
	let err = subscribe(&h, user, &service, "100").await.unwrap_err();
	assert!(matches!(err, domain::error::DomainError::NotFound { entity: "allocation", .. }), "got {err:?}");

	// And it must be refused BEFORE any money moves — no claim spent, no service claim born.
	h.relay.drain().await;
	let user_claim = h.ledger.balance(&LedgerAccountKey::UserClaim(user)).await.unwrap();
	assert_eq!(Usdt::from_base_units(user_claim.available()), usdt("100"), "the user's balance is untouched");
	let service_claim = h.ledger.balance(&LedgerAccountKey::ServiceClaim(service.clone())).await.unwrap();
	assert_eq!(Usdt::from_base_units(service_claim.posted), Usdt::ZERO, "no phantom fund claim was created");
}

#[tokio::test]
async fn a_draft_allocation_takes_no_money_until_opened() {
	let Some(h) = harness().await else { return };
	let user = UserId::new();
	let service = unique_service();
	fund_user(&h, user, "100").await;
	register(&h, &service).await;

	// Registered but still `draft` — listing and funding are separate operator decisions.
	assert!(subscribe(&h, user, &service, "50").await.is_err(), "a draft must not accept money");

	h.allocations.open(&service).await.unwrap();
	subscribe(&h, user, &service, "50").await.unwrap();
	h.relay.drain().await;
	let held = h.ledger.balance(&LedgerAccountKey::UserShares(service.clone(), user)).await.unwrap();
	assert_eq!(Shares::from_base_units(held.posted), shares("50"), "units minted at the seed NAV once open");
}

#[tokio::test]
async fn closing_stops_new_money_but_never_traps_an_investor() {
	let Some(h) = harness().await else { return };
	let user = UserId::new();
	let service = unique_service();
	fund_user(&h, user, "100").await;
	register(&h, &service).await;
	h.allocations.open(&service).await.unwrap();
	subscribe(&h, user, &service, "100").await.unwrap();
	h.relay.drain().await;

	h.allocations.close(&service).await.unwrap();
	assert!(subscribe(&h, user, &service, "1").await.is_err(), "a closed allocation refuses new subscriptions");

	// The asymmetry that matters: the investor can still get out. Refusing here would
	// lock 100 units of real money inside a wound-down product.
	let redemption = funds_app::request_redemption(&fund_ports(&h), &h.reds, user, service.clone(), shares("100"), now_unix())
		.await
		.expect("a closed allocation must still redeem");
	assert_eq!(redemption.service(), &service);
}

#[tokio::test]
async fn registering_twice_is_a_conflict_not_a_silent_reset() {
	let Some(h) = harness().await else { return };
	let service = unique_service();
	register(&h, &service).await;
	h.allocations.open(&service).await.unwrap();

	let mut duplicate = Allocation::register(AllocationId::new(), service.clone(), "Impostor", "", AllocationIcon::Venture).unwrap();
	let err = h.allocations.register(&mut duplicate).await.unwrap_err();
	assert!(matches!(err, domain::error::DomainError::Conflict(_)), "got {err:?}");

	// The live product kept its identity and, crucially, its open state.
	let current = h.allocations.find(&service).await.unwrap().unwrap();
	assert_eq!(current.title(), "EV Trading");
	assert_eq!(current.state(), AllocationState::Open);
	assert_eq!(current.icon(), AllocationIcon::default(), "the impostor's icon did not overwrite the live card");
}

#[tokio::test]
async fn transitions_persist_and_are_idempotent() {
	let Some(h) = harness().await else { return };
	let service = unique_service();
	register(&h, &service).await;

	assert_eq!(h.allocations.open(&service).await.unwrap().state(), AllocationState::Open);
	assert_eq!(h.allocations.open(&service).await.unwrap().state(), AllocationState::Open, "re-opening is a no-op");
	assert_eq!(h.allocations.close(&service).await.unwrap().state(), AllocationState::Closed);
	assert_eq!(h.allocations.close(&service).await.unwrap().state(), AllocationState::Closed, "re-closing is a no-op");
	assert_eq!(h.allocations.open(&service).await.unwrap().state(), AllocationState::Open, "closed reopens");

	let updated = h.allocations.update_details(&service, " Renamed ", " New summary ", Some(AllocationIcon::Yield)).await.unwrap();
	assert_eq!(updated.title(), "Renamed");
	assert_eq!(updated.summary(), "New summary");
	assert_eq!(updated.state(), AllocationState::Open, "a details edit leaves state alone");
}

#[tokio::test]
async fn transitions_on_an_unregistered_service_are_not_found() {
	let Some(h) = harness().await else { return };
	let service = unique_service();
	assert!(h.allocations.find(&service).await.unwrap().is_none());
	for result in [
		h.allocations.open(&service).await,
		h.allocations.close(&service).await,
		h.allocations.update_details(&service, "x", "", Some(AllocationIcon::default())).await,
	] {
		assert!(matches!(result, Err(domain::error::DomainError::NotFound { entity: "allocation", .. })));
	}
}

#[tokio::test]
async fn the_catalog_hides_drafts_and_closed_from_investors() {
	let Some(h) = harness().await else { return };
	let draft = unique_service();
	let open = unique_service();
	let closed = unique_service();
	for service in [&draft, &open, &closed] {
		register(&h, service).await;
	}
	h.allocations.open(&open).await.unwrap();
	h.allocations.open(&closed).await.unwrap();
	h.allocations.close(&closed).await.unwrap();

	let listed: Vec<String> = h.allocations.list(false).await.unwrap().iter().map(|r| r.allocation.service().to_string()).collect();
	assert!(listed.contains(&open.to_string()), "an open allocation is in the investor catalog");
	assert!(!listed.contains(&draft.to_string()), "a draft is hidden");
	assert!(!listed.contains(&closed.to_string()), "a closed allocation is hidden");

	let all: Vec<String> = h.allocations.list(true).await.unwrap().iter().map(|r| r.allocation.service().to_string()).collect();
	for service in [&draft, &open, &closed] {
		assert!(all.contains(&service.to_string()), "include_unlisted surfaces {service}");
	}
}

#[tokio::test]
async fn registration_is_audited_but_never_reaches_the_relay() {
	let Some(h) = harness().await else { return };
	let service = unique_service();
	let allocation = register(&h, &service).await;

	// The fact is in the append-only audit log…
	let logged: i64 = sqlx::query_scalar("SELECT count(*) FROM event_log WHERE aggregate = 'allocation' AND aggregate_id = $1")
		.bind(allocation.id().raw())
		.fetch_one(&h.pool)
		.await
		.unwrap();
	assert_eq!(logged, 1);

	// …and deliberately NOT in the outbox: an allocation moves no value, so the relay has
	// no ledger op for it and would park the row.
	let relayed: i64 = sqlx::query_scalar("SELECT count(*) FROM outbox WHERE aggregate = 'allocation'").fetch_one(&h.pool).await.unwrap();
	assert_eq!(relayed, 0, "allocation events must never enter the outbox");
}

#[tokio::test]
async fn a_subscription_past_the_unit_cap_is_refused_before_any_money_moves() {
	let Some(h) = harness().await else { return };
	let user = UserId::new();
	let service = unique_service();
	fund_user(&h, user, "200").await;
	register(&h, &service).await;
	h.allocations.open(&service).await.unwrap();
	// Sized to a hundred units — the whole point of the cap is that "open" and "unbounded"
	// are different things.
	h.allocations.set_unit_cap(&service, shares("100")).await.unwrap();

	// At the seed NAV of 1.0 this mints exactly the cap. Landing on it is allowed.
	subscribe(&h, user, &service, "100").await.unwrap();
	h.relay.drain().await;

	let err = subscribe(&h, user, &service, "1").await.unwrap_err();
	assert!(matches!(err, domain::error::DomainError::Validation(ref m) if m.contains("unit cap")), "got {err:?}");

	// Refused before the ledger, like the registry gate above it: the remaining 100 USDT is
	// still the user's, and the fund did not quietly issue a 101st unit.
	h.relay.drain().await;
	let claim = h.ledger.balance(&LedgerAccountKey::UserClaim(user)).await.unwrap();
	assert_eq!(Usdt::from_base_units(claim.available()), usdt("100"), "the refused subscription spent nothing");
	let outstanding = h.ledger.balance(&LedgerAccountKey::SharesOutstanding(service.clone())).await.unwrap();
	assert_eq!(Shares::from_base_units(outstanding.posted), shares("100"), "supply stopped at the cap");
}

#[tokio::test]
async fn narrowing_the_cap_below_the_issued_supply_stops_issuance_without_trapping_anyone() {
	let Some(h) = harness().await else { return };
	let user = UserId::new();
	let service = unique_service();
	fund_user(&h, user, "100").await;
	register(&h, &service).await;
	h.allocations.open(&service).await.unwrap();
	subscribe(&h, user, &service, "100").await.unwrap();
	h.relay.drain().await;

	// An operator decides the product has run further than intended and pulls the cap in
	// under what is already out. Legal, and it means "no more units" — not "some units are
	// now invalid".
	h.allocations.set_unit_cap(&service, shares("10")).await.unwrap();
	assert!(subscribe(&h, user, &service, "1").await.is_err(), "no further issuance");

	// The same asymmetry the lifecycle gate has: the holder still gets out in full.
	funds_app::request_redemption(&fund_ports(&h), &h.reds, user, service.clone(), shares("100"), now_unix())
		.await
		.expect("a narrowed cap must never block a redemption");
}

#[tokio::test]
async fn the_cap_defaults_on_registration_persists_and_is_reported_with_the_nav() {
	let Some(h) = harness().await else { return };
	let service = unique_service();
	register(&h, &service).await;
	h.allocations.open(&service).await.unwrap();

	// Registration lands on the default rather than on "unset", so the registry always has
	// a number to show and to refuse against.
	assert_eq!(h.allocations.find(&service).await.unwrap().unwrap().unit_cap(), domain::allocations::DEFAULT_UNIT_CAP);

	h.allocations.set_unit_cap(&service, shares("1000")).await.unwrap();
	assert_eq!(h.allocations.find(&service).await.unwrap().unwrap().unit_cap(), shares("1000"), "the new cap survives a reload");
	// Zero is refused — it would be indistinguishable from unset while silently closing the
	// product to new money.
	assert!(h.allocations.set_unit_cap(&service, Shares::ZERO).await.is_err());

	// And the read side the screens use reports the headroom, so a client never offers
	// capacity the subscribe gate would then refuse.
	let view = funds_app::fund_nav_view(&h.allocations, &h.nav, h.ledger.as_ref(), service.clone(), now_unix()).await.unwrap();
	assert_eq!(view.unit_cap, shares("1000"));
	assert_eq!(view.remaining_capacity, shares("1000"), "nothing issued yet, so the whole cap is available");
}

#[tokio::test]
async fn a_valuation_cannot_be_posted_for_an_unregistered_service() {
	let Some(h) = harness().await else { return };
	let service = unique_service();
	// The other door into a phantom fund: an AUM post would write a valuation history for
	// a service no registry entry backs.
	let err = funds_app::post_fund_valuation(&h.allocations, &h.nav, h.ledger.as_ref(), service.clone(), usdt("100"), "op", true)
		.await
		.unwrap_err();
	assert!(matches!(err, domain::error::DomainError::NotFound { entity: "allocation", .. }), "got {err:?}");
}

#[tokio::test]
async fn the_icon_is_stored_survives_a_reload_and_is_editable() {
	let Some(h) = harness().await else { return };
	let service = unique_service();

	// Registration carries the operator's pick all the way to the column — the point of
	// the field is that the catalog card is a decision, not a letter derived from the title.
	register_with_icon(&h, &service, AllocationIcon::RealEstate).await;
	assert_eq!(h.allocations.find(&service).await.unwrap().unwrap().icon(), AllocationIcon::RealEstate);

	// It is presentation, so editing it is an ordinary details edit and leaves the
	// lifecycle alone — a live product can be restyled without touching money flow.
	h.allocations.open(&service).await.unwrap();
	let updated = h
		.allocations
		.update_details(&service, "EV Arbitrage", "Cross-venue basis", Some(AllocationIcon::Arbitrage))
		.await
		.unwrap();
	assert_eq!(updated.icon(), AllocationIcon::Arbitrage);
	assert_eq!(updated.state(), AllocationState::Open);
	assert_eq!(
		h.allocations.find(&service).await.unwrap().unwrap().icon(),
		AllocationIcon::Arbitrage,
		"the new icon survives a reload"
	);

	// The catalog read carries it too — that projection is what the cabinet renders.
	let listed = h.allocations.list(true).await.unwrap();
	let row = listed.iter().find(|r| r.allocation.service() == &service).expect("the registered product is in the catalog");
	assert_eq!(row.allocation.icon(), AllocationIcon::Arbitrage);

	// A registration that names nothing lands on the default rather than on NULL, so the
	// client always has a glyph to draw.
	let plain = unique_service();
	register(&h, &plain).await;
	assert_eq!(h.allocations.find(&plain).await.unwrap().unwrap().icon(), AllocationIcon::Fund);
}

#[tokio::test]
async fn a_row_written_by_a_pod_that_predates_the_icon_column_reads_back_as_the_default() {
	let Some(h) = harness().await else { return };
	let service = unique_service();

	// Byte for byte the INSERT the CURRENTLY DEPLOYED code runs: it names its columns,
	// and `icon` is not among them. That is the claim migration 0027's header makes about
	// rolling deploys, and until now nothing checked it — the previous test wrote
	// `icon = DEFAULT`, which is a literal 'fund' spelled differently, so it proved
	// `parse("fund") == Fund` (already a unit test) and could not fail for the reason it
	// was named after.
	sqlx::query("INSERT INTO allocations (id, service, title, summary, state, unit_cap) VALUES ($1, $2, $3, $4, $5, $6)")
		.bind(Uuid::new_v4())
		.bind(service.as_str())
		.bind("Legacy Fund")
		.bind("Registered by a pod that had never heard of icons")
		.bind("open")
		.bind(domain::allocations::DEFAULT_UNIT_CAP.base_units().to_string())
		.execute(&h.pool)
		.await
		.expect("the old INSERT must keep working — a migration that breaks it breaks the rolling deploy");

	let stored: String = sqlx::query_scalar("SELECT icon FROM allocations WHERE service = $1")
		.bind(service.as_str())
		.fetch_one(&h.pool)
		.await
		.unwrap();
	assert_eq!(stored, "fund", "the column DEFAULT is what fills the gap the old writer leaves");
	assert_eq!(h.allocations.find(&service).await.unwrap().unwrap().icon(), AllocationIcon::Fund);
	// And it is in the catalog the cabinet renders, not just readable one row at a time.
	let listed = h.allocations.list(true).await.unwrap();
	let row = listed.iter().find(|r| r.allocation.service() == &service).expect("the legacy row is in the catalog");
	assert_eq!(row.allocation.icon(), AllocationIcon::Fund);
}

#[tokio::test]
async fn every_icon_the_domain_knows_is_accepted_by_the_column() {
	let Some(h) = harness().await else { return };
	let service = unique_service();
	register(&h, &service).await;

	// The pair nothing covered: `domain_icons_match_the_wire_contract` compares the domain
	// enum with the wire contract and never touches SQL, so a variant added to the enum,
	// to `icon::ALL` and to the client but FORGOTTEN in a migration compiled, unit-tested
	// green, and first showed up in front of an operator — as a CHECK violation rolling
	// back the whole `update_details` transaction, taking the title edit with it and
	// answering `internal` where a validation error belonged.
	for icon in AllocationIcon::ALL {
		sqlx::query("UPDATE allocations SET icon = $2 WHERE service = $1")
			.bind(service.as_str())
			.bind(icon.as_str())
			.execute(&h.pool)
			.await
			.unwrap_or_else(|err| panic!("the column refuses {icon:?}, which the domain calls legal — migration 0027 is missing it: {err}"));
		assert_eq!(h.allocations.find(&service).await.unwrap().unwrap().icon(), icon, "{icon:?} did not survive the round trip");
	}
}

#[tokio::test]
async fn the_database_refuses_an_icon_outside_the_vocabulary() {
	let Some(h) = harness().await else { return };
	let service = unique_service();
	register(&h, &service).await;

	// The CHECK in 0027 is the backstop under `AllocationIcon::parse`: a hand-written
	// UPDATE cannot leave a row outside the vocabulary this build ships artwork for.
	let err = sqlx::query("UPDATE allocations SET icon = 'rocket' WHERE service = $1")
		.bind(service.as_str())
		.execute(&h.pool)
		.await
		.unwrap_err();
	// Identified by SQLSTATE and constraint name rather than by
	// `err.to_string().contains("icon")`, which any error naming the column would have
	// satisfied — a NOT NULL violation, a type mismatch, a typo in the query text.
	let db_err = err.as_database_error().expect("a server-side error, not a client-side one");
	assert_eq!(db_err.code().as_deref(), Some("23514"), "23514 is check_violation; got {err}");
	assert_eq!(db_err.constraint(), Some("allocations_icon_check"), "refused by some other constraint: {err}");
}

#[tokio::test]
async fn an_icon_from_a_wider_vocabulary_reads_back_as_the_default_instead_of_failing() {
	let Some(h) = harness().await else { return };
	let service = unique_service();

	// Widening the vocabulary is an ordinary migration: ship it, let an operator pick the
	// new value, then roll the release back — and this build is reading a string it cannot
	// parse. Strict parsing on the read path turned that into a failing `list()` (the
	// whole catalog) and a failing `find()`, which is the gate every subscribe and redeem
	// passes through. A presentation column must not be able to stop money.
	//
	// Producing such a row needs a table whose CHECK is wider than this build's enum, and
	// that table is built in a throwaway schema rather than by dropping the real
	// constraint: `allocations` is shared with every other test in this binary, and one
	// running while the CHECK was off would assert against a schema that no longer
	// enforces anything — a vacuous pass, which is the exact failure this whole area is
	// about. `LIKE ... INCLUDING DEFAULTS` copies the columns, their NOT NULLs and the
	// `icon` default, and deliberately not the CHECK.
	let url = std::env::var("DATABASE_URL").unwrap();
	let schema = format!("icon_probe_{}", Uuid::new_v4().simple());
	sqlx::query(AssertSqlSafe(format!("CREATE SCHEMA {schema}"))).execute(&h.pool).await.unwrap();
	sqlx::query(AssertSqlSafe(format!("CREATE TABLE {schema}.allocations (LIKE public.allocations INCLUDING DEFAULTS)")))
		.execute(&h.pool)
		.await
		.unwrap();
	sqlx::query(AssertSqlSafe(format!(
		"INSERT INTO {schema}.allocations (id, service, title, summary, state, unit_cap, icon) VALUES ($1, $2, $3, $4, 'open', $5, 'hologram')"
	)))
	.bind(Uuid::new_v4())
	.bind(service.as_str())
	.bind("EV Hologram")
	.bind("Registered by a build that knew one more icon")
	.bind(domain::allocations::DEFAULT_UNIT_CAP.base_units().to_string())
	.execute(&h.pool)
	.await
	.unwrap();

	// The adapter's SQL names `allocations` unqualified, so a pool whose `search_path`
	// leads with the probe schema reads the wider table through the ordinary code path —
	// the same `find`/`list` the money plane calls, not a re-implementation of them.
	let options = url.parse::<PgConnectOptions>().unwrap().options([("search_path", format!("{schema},public"))]);
	let probe = PgPoolOptions::new().max_connections(2).connect_with(options).await.unwrap();
	let allocations = PgAllocations::new(probe.clone());

	let found = allocations.find(&service).await;
	let listed = allocations.list(true).await;
	probe.close().await;
	sqlx::query(AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE"))).execute(&h.pool).await.unwrap();

	let found = found.expect("an unparseable icon must not fail the read every subscribe depends on");
	assert_eq!(
		found.expect("the row is there").icon(),
		AllocationIcon::Fund,
		"an icon this build cannot draw degrades to the default, matching what the client already does"
	);
	let listed = listed.expect("nor may it fail the catalog read for every other product");
	let row = listed.iter().find(|r| r.allocation.service() == &service).expect("the row is still listed, wearing the default");
	assert_eq!(row.allocation.icon(), AllocationIcon::Fund);
}

#[tokio::test]
async fn an_update_that_names_no_icon_keeps_the_one_the_operator_picked() {
	let Some(h) = harness().await else { return };
	let service = unique_service();
	register_with_icon(&h, &service, AllocationIcon::RealEstate).await;

	// `None` is what an `UpdateAllocation` with the `optional` field unset reaches the
	// registry as: an older consumer repo, a browser tab on a stale bundle, a pod still
	// running the previous release. Before the field carried presence they all sent the
	// proto3 default — an empty string, indistinguishable from a deliberate clear — and a
	// plain title edit silently restyled a live product back to `fund`.
	let renamed = h.allocations.update_details(&service, "EV Property", "Income-producing property", None).await.unwrap();
	assert_eq!(renamed.title(), "EV Property");
	assert_eq!(renamed.icon(), AllocationIcon::RealEstate, "an absent icon left the operator's pick alone");
	assert_eq!(
		h.allocations.find(&service).await.unwrap().unwrap().icon(),
		AllocationIcon::RealEstate,
		"and the column was not overwritten either"
	);

	// `Some` still sets it, so the field has not become unwritable…
	let restyled = h.allocations.update_details(&service, "EV Property", "", Some(AllocationIcon::Venture)).await.unwrap();
	assert_eq!(restyled.icon(), AllocationIcon::Venture);
	// …and `Some(default)` is how a reset to the neutral glyph stays reachable, which is
	// exactly what absence must NOT mean.
	let reset = h.allocations.update_details(&service, "EV Property", "", Some(AllocationIcon::default())).await.unwrap();
	assert_eq!(reset.icon(), AllocationIcon::Fund);
	assert_eq!(h.allocations.find(&service).await.unwrap().unwrap().icon(), AllocationIcon::Fund);
}
