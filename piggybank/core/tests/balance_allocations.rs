//! Integration tests for the money plane — real Postgres **and** TigerBeetle (no
//! mocks, per the project rules). They run when `DATABASE_URL` is set and a
//! TigerBeetle replica is reachable (e.g. `nix run .#db` + `.#tb`), and skip
//! otherwise so a DB-less `cargo test` still passes. Each test uses fresh random
//! ids (user/service), so runs are isolated on shared infrastructure. The relay is
//! driven explicitly via `Relay::drain` to apply committed events deterministically.
//!
//! Boundary authz (`require_admin`/`caller_id`) is the same path the live `UsersSvc`
//! already uses; the load-bearing money invariant — the revoke rule — is exercised
//! here at the aggregate/repository layer.

use std::sync::Arc;

use domain::{
	balance::{LedgerAccountKey, Party, ServiceId, TransferCode},
	money::{Nav, Network, Shares, TxRef, Usdt},
	redemptions::RedemptionState,
	users::UserId,
};
use piggybank_core::{
	application::{balance as balance_app, funds as funds_app},
	infrastructure::{
		custody::StubCustody,
		db,
		ledger::{self, TbLedger},
		nav::PgNav,
		redemptions::PgRedemptions,
		relay::Relay,
		subscriptions::PgSubscriptions,
		tigerbeetle::TigerBeetle,
	},
	ports::ledger::{Ledger, LedgerError, LedgerTransfer},
};
use sqlx::PgPool;
use tokio::sync::Notify;
use uuid::Uuid;

struct Harness {
	pool: PgPool,
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
	// If TigerBeetle isn't actually reachable, the first real op fails — skip then.
	if ledger::seed_singletons(ledger.as_ref()).await.is_err() {
		eprintln!("TigerBeetle unreachable — skipping money-plane test");
		return None;
	}

	let notify = Arc::new(Notify::new());
	let relay = Relay::new(pool.clone(), ledger.clone(), Arc::new(StubCustody), notify.clone());
	Some(Harness { pool, ledger, relay, notify })
}

fn usdt(decimal: &str) -> Usdt {
	Usdt::parse_decimal(decimal).unwrap()
}

fn unique_service() -> ServiceId {
	ServiceId::parse(&format!("svc-{}", Uuid::new_v4())).unwrap()
}

fn unique_tx_ref() -> TxRef {
	TxRef::parse(&format!("itest-{}", Uuid::new_v4())).unwrap()
}

async fn claim(h: &Harness, key: &LedgerAccountKey) -> Usdt {
	Usdt::from_base_units(h.ledger.balance(key).await.unwrap().posted)
}

fn shares(decimal: &str) -> Shares {
	Shares::parse_decimal(decimal).unwrap()
}

async fn units(h: &Harness, key: &LedgerAccountKey) -> Shares {
	Shares::from_base_units(h.ledger.balance(key).await.unwrap().posted)
}

async fn units_available(h: &Harness, key: &LedgerAccountKey) -> Shares {
	Shares::from_base_units(h.ledger.balance(key).await.unwrap().available())
}

fn now_unix() -> i64 {
	use std::time::{SystemTime, UNIX_EPOCH};
	SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
}

#[tokio::test]
async fn deposit_credits_once_and_is_idempotent_by_tx_ref() {
	let Some(h) = harness().await else { return };
	let user = UserId::new();
	let network = Network::Trc20;
	let key = LedgerAccountKey::UserClaim(user);
	let tx_ref = unique_tx_ref();

	assert!(claim(&h, &key).await.is_zero());

	let recorded = balance_app::record_deposit(&h.pool, &h.notify, tx_ref.clone(), Party::User(user), network, usdt("100"))
		.await
		.unwrap();
	assert!(recorded, "first record is new");
	h.relay.drain().await;
	assert_eq!(claim(&h, &key).await, usdt("100"), "the deposit credited the user's claim");

	// Re-recording the same chain tx is a no-op — no second event, no double credit.
	let again = balance_app::record_deposit(&h.pool, &h.notify, tx_ref, Party::User(user), network, usdt("100")).await.unwrap();
	assert!(!again, "duplicate tx_ref is idempotent");
	h.relay.drain().await;
	assert_eq!(claim(&h, &key).await, usdt("100"), "no double credit on a duplicate");
}

#[tokio::test]
async fn deposit_credits_a_claim_backed_by_custody() {
	let Some(h) = harness().await else { return };
	let network = Network::Ton;
	let wallet = LedgerAccountKey::CryptoWallet(network);
	let user = UserId::new();

	balance_app::record_deposit(&h.pool, &h.notify, unique_tx_ref(), Party::User(user), network, usdt("250"))
		.await
		.unwrap();
	h.relay.drain().await;

	// The user's unified claim is isolated (random user); the rail's custody wallet is a
	// shared singleton. Assert the direction that holds regardless of concurrent
	// deposits: the rail that funded this claim backs it (global sum custody >= claims).
	let user_claim = claim(&h, &LedgerAccountKey::UserClaim(user)).await;
	assert_eq!(user_claim, usdt("250"), "the deposit credited the user's claim");
	assert!(claim(&h, &wallet).await >= user_claim, "custody backs the claim (sum custody >= claims)");
}

#[tokio::test]
async fn non_negative_flag_is_the_ledger_backstop() {
	let Some(h) = harness().await else { return };
	let user = UserId::new();
	let network = Network::Bep20;
	balance_app::record_deposit(&h.pool, &h.notify, unique_tx_ref(), Party::User(user), network, usdt("10"))
		.await
		.unwrap();
	h.relay.drain().await;

	// Bypass the application check and over-debit the claim directly: TB's
	// DebitsMustNotExceedCredits flag rejects it as InsufficientFunds.
	let transfer = LedgerTransfer {
		id: Uuid::new_v4().as_u128(),
		debit: LedgerAccountKey::UserClaim(user),
		credit: LedgerAccountKey::ServiceClaim(unique_service()),
		amount: usdt("50").base_units(),
		code: TransferCode::UserAllocate,
		reference: 0,
	};
	let err = h.ledger.post(&transfer).await.unwrap_err();
	assert!(matches!(err, LedgerError::InsufficientFunds), "the flag rejected the over-debit, got {err:?}");
}

#[tokio::test]
async fn transfer_id_is_idempotent_no_double_move() {
	let Some(h) = harness().await else { return };
	let user = UserId::new();
	let network = Network::Ton;
	let service = unique_service();
	balance_app::record_deposit(&h.pool, &h.notify, unique_tx_ref(), Party::User(user), network, usdt("100"))
		.await
		.unwrap();
	h.relay.drain().await;

	let transfer = LedgerTransfer {
		id: Uuid::new_v4().as_u128(),
		debit: LedgerAccountKey::UserClaim(user),
		credit: LedgerAccountKey::ServiceClaim(service.clone()),
		amount: usdt("30").base_units(),
		code: TransferCode::UserAllocate,
		reference: 0,
	};
	// Same deterministic id twice ⇒ the second is `Exists` ⇒ applied exactly once.
	h.ledger.post(&transfer).await.unwrap();
	h.ledger.post(&transfer).await.unwrap();

	assert_eq!(claim(&h, &LedgerAccountKey::ServiceClaim(service)).await, usdt("30"), "no double move");
	assert_eq!(claim(&h, &LedgerAccountKey::UserClaim(user)).await, usdt("70"));
}

#[tokio::test]
async fn share_ledger_mints_burns_and_rejects_over_redeem() {
	let Some(h) = harness().await else { return };
	let user = UserId::new();
	let service = unique_service();
	let user_shares = LedgerAccountKey::UserShares(service.clone(), user);
	let outstanding = LedgerAccountKey::SharesOutstanding(service.clone());

	// Mint 100 units on the Share ledger: Dr UserShares / Cr SharesOutstanding.
	let mint = LedgerTransfer {
		id: Uuid::new_v4().as_u128(),
		debit: user_shares.clone(),
		credit: outstanding.clone(),
		amount: shares("100").base_units(),
		code: TransferCode::ShareMint,
		reference: 0,
	};
	h.ledger.post(&mint).await.unwrap();
	// Per-service invariant: SharesOutstanding == sum(UserShares). This (user, service) is
	// fresh, so both equal the minted amount exactly.
	assert_eq!(units(&h, &user_shares).await, shares("100"), "the user holds the minted units");
	assert_eq!(units(&h, &outstanding).await, shares("100"), "supply equals the user's holding");

	// Over-redeem: a *pending* burn of 150 (Dr SharesOutstanding / Cr UserShares) exceeds
	// the 100 minted — TigerBeetle rejects it atomically via the non-negative flag, even as
	// a reservation. The TB flag, not a row-lock, is the money backstop.
	let over_burn = LedgerTransfer {
		id: Uuid::new_v4().as_u128(),
		debit: outstanding.clone(),
		credit: user_shares.clone(),
		amount: shares("150").base_units(),
		code: TransferCode::ShareBurn,
		reference: 0,
	};
	let err = h.ledger.reserve(&over_burn).await.unwrap_err();
	assert!(matches!(err, LedgerError::InsufficientFunds), "TB rejects the over-redeem reserve, got {err:?}");

	// A burn within the holding posts and reduces both sides equally — the invariant holds.
	let burn = LedgerTransfer {
		id: Uuid::new_v4().as_u128(),
		debit: outstanding.clone(),
		credit: user_shares.clone(),
		amount: shares("40").base_units(),
		code: TransferCode::ShareBurn,
		reference: 0,
	};
	h.ledger.post(&burn).await.unwrap();
	assert_eq!(units(&h, &user_shares).await, shares("60"), "the user's units dropped by the burn");
	assert_eq!(units(&h, &outstanding).await, shares("60"), "supply dropped in lockstep");
}

#[tokio::test]
async fn fund_valuation_derives_nav_and_guards_fat_finger() {
	let Some(h) = harness().await else { return };
	let nav_repo = PgNav::new(h.pool.clone());
	let user = UserId::new();
	let service = unique_service();

	// No mark yet → the current NAV is the bootstrap seed (1.0).
	assert_eq!(funds_app::current_nav(&nav_repo, &service).await.unwrap(), Nav::SEED);
	// Posting AUM with zero units outstanding is rejected — NAV is undefined.
	assert!(
		funds_app::post_fund_valuation(&nav_repo, h.ledger.as_ref(), service.clone(), usdt("100"), "op", false)
			.await
			.is_err()
	);

	// Mint 100 units so SharesOutstanding(service) = 100, then NAV can be derived.
	let mint = LedgerTransfer {
		id: Uuid::new_v4().as_u128(),
		debit: LedgerAccountKey::UserShares(service.clone(), user),
		credit: LedgerAccountKey::SharesOutstanding(service.clone()),
		amount: shares("100").base_units(),
		code: TransferCode::ShareMint,
		reference: 0,
	};
	h.ledger.post(&mint).await.unwrap();

	// AUM 150 over 100 units → NAV 1.5, and it becomes the current price.
	let v = funds_app::post_fund_valuation(&nav_repo, h.ledger.as_ref(), service.clone(), usdt("150"), "op", false)
		.await
		.unwrap();
	assert_eq!(v.nav, Nav::parse_decimal("1.5").unwrap());
	assert_eq!(funds_app::current_nav(&nav_repo, &service).await.unwrap(), Nav::parse_decimal("1.5").unwrap());

	// A 10x fat-finger (AUM 1500 → NAV 15, +900%) is rejected without override…
	assert!(
		funds_app::post_fund_valuation(&nav_repo, h.ledger.as_ref(), service.clone(), usdt("1500"), "op", false)
			.await
			.is_err()
	);
	// …and accepted with it.
	let forced = funds_app::post_fund_valuation(&nav_repo, h.ledger.as_ref(), service.clone(), usdt("1500"), "op", true)
		.await
		.unwrap();
	assert_eq!(forced.nav, Nav::parse_decimal("15").unwrap());
}

#[tokio::test]
async fn subscribe_mints_units_moves_cash_and_prices_at_nav() {
	let Some(h) = harness().await else { return };
	let subs = PgSubscriptions::new(h.pool.clone());
	let nav_repo = PgNav::new(h.pool.clone());
	let user = UserId::new();
	let service = unique_service();
	let now = now_unix();
	let user_claim = LedgerAccountKey::UserClaim(user);
	let service_claim = LedgerAccountKey::ServiceClaim(service.clone());
	let user_shares = LedgerAccountKey::UserShares(service.clone(), user);
	let outstanding = LedgerAccountKey::SharesOutstanding(service.clone());

	balance_app::record_deposit(&h.pool, &h.notify, unique_tx_ref(), Party::User(user), Network::Bep20, usdt("400"))
		.await
		.unwrap();
	h.relay.drain().await;

	// First subscription mints at the seed NAV (1.0): 200 cash → 200 units.
	funds_app::subscribe(&subs, h.ledger.as_ref(), &nav_repo, &h.notify, user, service.clone(), usdt("200"), now)
		.await
		.unwrap();
	h.relay.drain().await;
	assert_eq!(claim(&h, &user_claim).await, usdt("200"), "cash left the user's claim");
	assert_eq!(claim(&h, &service_claim).await, usdt("200"), "cash entered the fund pool");
	assert_eq!(units(&h, &user_shares).await, shares("200"), "units minted to the user");
	assert_eq!(units(&h, &outstanding).await, shares("200"), "supply grew with the mint");

	// Operator marks the fund up to NAV 2.0 (AUM 400 over 200 units).
	funds_app::post_fund_valuation(&nav_repo, h.ledger.as_ref(), service.clone(), usdt("400"), "op", false)
		.await
		.unwrap();

	// A second subscription prices at NAV 2.0: 100 cash → 50 units (fractional pricing).
	funds_app::subscribe(&subs, h.ledger.as_ref(), &nav_repo, &h.notify, user, service.clone(), usdt("100"), now)
		.await
		.unwrap();
	h.relay.drain().await;
	assert_eq!(units(&h, &user_shares).await, shares("250"), "50 more units at NAV 2");
	assert_eq!(claim(&h, &user_claim).await, usdt("100"), "cash dropped by the second subscription");

	// Dealing on a stale mark (now far past the last valuation) is refused — the user has
	// the balance, so this isolates the staleness guard.
	let stale_now = now + funds_app::MAX_NAV_AGE_SECS + 100;
	assert!(
		funds_app::subscribe(&subs, h.ledger.as_ref(), &nav_repo, &h.notify, user, service.clone(), usdt("50"), stale_now)
			.await
			.is_err()
	);

	// Subscribing beyond the available balance is rejected Read-First.
	assert!(
		funds_app::subscribe(&subs, h.ledger.as_ref(), &nav_repo, &h.notify, user, service.clone(), usdt("1000"), now)
			.await
			.is_err()
	);
}

#[tokio::test]
async fn redeem_when_fund_is_liquid_auto_completes() {
	let Some(h) = harness().await else { return };
	let subs = PgSubscriptions::new(h.pool.clone());
	let reds = PgRedemptions::new(h.pool.clone());
	let nav_repo = PgNav::new(h.pool.clone());
	let user = UserId::new();
	let service = unique_service();
	let now = now_unix();
	let user_claim = LedgerAccountKey::UserClaim(user);
	let service_claim = LedgerAccountKey::ServiceClaim(service.clone());
	let user_shares = LedgerAccountKey::UserShares(service.clone(), user);
	let outstanding = LedgerAccountKey::SharesOutstanding(service.clone());

	balance_app::record_deposit(&h.pool, &h.notify, unique_tx_ref(), Party::User(user), Network::Bep20, usdt("100"))
		.await
		.unwrap();
	h.relay.drain().await;
	funds_app::subscribe(&subs, h.ledger.as_ref(), &nav_repo, &h.notify, user, service.clone(), usdt("100"), now)
		.await
		.unwrap();
	h.relay.drain().await;

	// Redeem 40 units at the seed NAV (1.0) → 40 cash. The fund claim (100) covers it, so
	// it auto-settles in one request (a separate settle command, not a co-emitted event).
	let r = funds_app::request_redemption(&reds, h.ledger.as_ref(), &nav_repo, &h.notify, user, service.clone(), shares("40"), now)
		.await
		.unwrap();
	assert_eq!(r.state(), RedemptionState::Completed, "a liquid redemption settles immediately");
	h.relay.drain().await;

	assert_eq!(units(&h, &user_shares).await, shares("60"), "units burned");
	assert_eq!(units(&h, &outstanding).await, shares("60"), "supply dropped with the burn");
	assert_eq!(claim(&h, &service_claim).await, usdt("60"), "cash left the fund pool");
	assert_eq!(claim(&h, &user_claim).await, usdt("40"), "cash credited to the user");
}

/// Set a user up holding 100 units in a fund marked up to NAV 2 — so a full redemption
/// values to 200 cash while the fund claim holds only 100 — then request it, returning the
/// queued redemption id. Shared by the short-fund tests.
async fn queued_short_redemption(
	h: &Harness,
	subs: &PgSubscriptions,
	reds: &PgRedemptions,
	nav_repo: &PgNav,
	user: UserId,
	service: &ServiceId,
	now: i64,
) -> domain::redemptions::RedemptionId {
	balance_app::record_deposit(&h.pool, &h.notify, unique_tx_ref(), Party::User(user), Network::Bep20, usdt("100"))
		.await
		.unwrap();
	h.relay.drain().await;
	funds_app::subscribe(subs, h.ledger.as_ref(), nav_repo, &h.notify, user, service.clone(), usdt("100"), now)
		.await
		.unwrap();
	h.relay.drain().await;
	// Mark up to NAV 2 (AUM 200 over 100 units) — a +100% move, so the operator forces it.
	funds_app::post_fund_valuation(nav_repo, h.ledger.as_ref(), service.clone(), usdt("200"), "op", true)
		.await
		.unwrap();
	let r = funds_app::request_redemption(reds, h.ledger.as_ref(), nav_repo, &h.notify, user, service.clone(), shares("100"), now)
		.await
		.unwrap();
	assert_eq!(r.state(), RedemptionState::Queued, "a short fund queues the redemption");
	h.relay.drain().await;
	r.id()
}

#[tokio::test]
async fn redeem_on_a_short_fund_queues_then_settles_with_profit() {
	let Some(h) = harness().await else { return };
	let subs = PgSubscriptions::new(h.pool.clone());
	let reds = PgRedemptions::new(h.pool.clone());
	let nav_repo = PgNav::new(h.pool.clone());
	let user = UserId::new();
	let service = unique_service();
	let now = now_unix();
	let user_claim = LedgerAccountKey::UserClaim(user);
	let service_claim = LedgerAccountKey::ServiceClaim(service.clone());
	let user_shares = LedgerAccountKey::UserShares(service.clone(), user);

	let id = queued_short_redemption(&h, &subs, &reds, &nav_repo, user, &service, now).await;
	// Units are reserved (locked), not burned; no cash paid while queued.
	assert_eq!(units(&h, &user_shares).await, shares("100"), "units still posted (reserved, not burned)");
	assert_eq!(units_available(&h, &user_shares).await, Shares::ZERO, "units locked by the reservation");
	assert_eq!(claim(&h, &user_claim).await, Usdt::ZERO, "no cash paid while queued");

	// The fund realizes profit: a deposit credits the service claim up to 200.
	balance_app::record_deposit(&h.pool, &h.notify, unique_tx_ref(), Party::Service(service.clone()), Network::Bep20, usdt("100"))
		.await
		.unwrap();
	h.relay.drain().await;
	assert_eq!(claim(&h, &service_claim).await, usdt("200"), "the fund topped up");

	// Operator settles — priced at the settle-time NAV (2) and paid in full.
	let settled = funds_app::settle_redemption(&reds, h.ledger.as_ref(), &nav_repo, &h.notify, id, now).await.unwrap();
	assert_eq!(settled.state(), RedemptionState::Completed);
	h.relay.drain().await;

	assert_eq!(units(&h, &user_shares).await, Shares::ZERO, "all units burned at settle");
	assert_eq!(claim(&h, &service_claim).await, Usdt::ZERO, "the fund paid out");
	assert_eq!(claim(&h, &user_claim).await, usdt("200"), "100 in, 200 out — NAV doubled, profit realized");
}

#[tokio::test]
async fn settling_a_short_fund_parks_without_burning_or_paying() {
	let Some(h) = harness().await else { return };
	let subs = PgSubscriptions::new(h.pool.clone());
	let reds = PgRedemptions::new(h.pool.clone());
	let nav_repo = PgNav::new(h.pool.clone());
	let user = UserId::new();
	let service = unique_service();
	let now = now_unix();
	let user_claim = LedgerAccountKey::UserClaim(user);
	let service_claim = LedgerAccountKey::ServiceClaim(service.clone());
	let user_shares = LedgerAccountKey::UserShares(service.clone(), user);

	let id = queued_short_redemption(&h, &subs, &reds, &nav_repo, user, &service, now).await;

	// Settle while the fund is STILL short (100 < 200) — the relay's payout pre-check parks
	// the whole event. Burn-first ordering means nothing is applied: no half-burn, no cash.
	funds_app::settle_redemption(&reds, h.ledger.as_ref(), &nav_repo, &h.notify, id, now).await.unwrap();
	h.relay.drain().await;
	assert_eq!(units(&h, &user_shares).await, shares("100"), "units NOT burned (settle parked)");
	assert_eq!(claim(&h, &user_claim).await, Usdt::ZERO, "no cash paid (settle parked)");
	assert_eq!(claim(&h, &service_claim).await, usdt("100"), "the fund claim is untouched");
}

#[tokio::test]
async fn cancelling_a_queued_redemption_returns_the_units() {
	let Some(h) = harness().await else { return };
	let subs = PgSubscriptions::new(h.pool.clone());
	let reds = PgRedemptions::new(h.pool.clone());
	let nav_repo = PgNav::new(h.pool.clone());
	let user = UserId::new();
	let service = unique_service();
	let now = now_unix();
	let user_shares = LedgerAccountKey::UserShares(service.clone(), user);

	let id = queued_short_redemption(&h, &subs, &reds, &nav_repo, user, &service, now).await;

	// A non-owner cannot cancel.
	assert!(funds_app::cancel_redemption(&reds, &h.notify, id, UserId::new()).await.is_err(), "only the owner may cancel");

	// The owner cancels — the relay voids the burn, releasing the locked units.
	let cancelled = funds_app::cancel_redemption(&reds, &h.notify, id, user).await.unwrap();
	assert_eq!(cancelled.state(), RedemptionState::Cancelled);
	h.relay.drain().await;
	assert_eq!(units(&h, &user_shares).await, shares("100"), "units still held");
	assert_eq!(units_available(&h, &user_shares).await, shares("100"), "the reservation lock was released");
}
