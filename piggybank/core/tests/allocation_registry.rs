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
	allocations::{Allocation, AllocationId, AllocationState},
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
use sqlx::PgPool;
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
	let mut allocation = Allocation::register(AllocationId::new(), service.clone(), "EV Trading", "Systematic crypto").unwrap();
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
	funds_app::subscribe(&h.allocations, &h.subs, h.ledger.as_ref(), &h.nav, &h.notify, user, service.clone(), usdt(amount), now_unix())
		.await
		.map(|_| ())
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
	let redemption = funds_app::request_redemption(&h.allocations, &h.reds, h.ledger.as_ref(), &h.nav, &h.notify, user, service.clone(), shares("100"), now_unix())
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

	let mut duplicate = Allocation::register(AllocationId::new(), service.clone(), "Impostor", "").unwrap();
	let err = h.allocations.register(&mut duplicate).await.unwrap_err();
	assert!(matches!(err, domain::error::DomainError::Conflict(_)), "got {err:?}");

	// The live product kept its identity and, crucially, its open state.
	let current = h.allocations.find(&service).await.unwrap().unwrap();
	assert_eq!(current.title(), "EV Trading");
	assert_eq!(current.state(), AllocationState::Open);
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

	let updated = h.allocations.update_details(&service, " Renamed ", " New summary ").await.unwrap();
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
		h.allocations.update_details(&service, "x", "").await,
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
