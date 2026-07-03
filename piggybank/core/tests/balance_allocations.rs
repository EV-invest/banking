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
	auth::AuthSubject,
	balance::{LedgerAccountKey, Party, ServiceId, TransferCode},
	money::{Nav, Network, Shares, TxRef, Usdt, WalletAddress},
	redemptions::RedemptionState,
	subscriptions::{Subscription, SubscriptionId},
	users::{Email, UserId},
};
use piggybank_core::{
	application::{balance as balance_app, funds as funds_app, withdrawals as withdrawal_app},
	infrastructure::{
		custody::StubCustody,
		db,
		deposits::PgDeposits,
		ledger::{self, TbLedger},
		nav::PgNav,
		positions::PgFundPositions,
		redemptions::PgRedemptions,
		relay::Relay,
		subscriptions::PgSubscriptions,
		tigerbeetle::TigerBeetle,
		users::PgUsers,
		withdrawals::PgWithdrawals,
	},
	ports::{
		FundPositionReader, SubscriptionRepository, UserRepository,
		ledger::{Ledger, LedgerError, LedgerTransfer},
		nav::NavMarks,
	},
};
use sqlx::PgPool;
use tokio::sync::Notify;
use uuid::Uuid;

struct Harness {
	pool: PgPool,
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
	// If TigerBeetle isn't actually reachable, the first real op fails — skip then.
	if ledger::seed_singletons(ledger.as_ref()).await.is_err() {
		eprintln!("TigerBeetle unreachable — skipping money-plane test");
		return None;
	}

	let notify = Arc::new(Notify::new());
	let relay = Relay::new(pool.clone(), ledger.clone(), Arc::new(StubCustody), notify.clone());
	let deposits = PgDeposits::new(pool.clone());
	Some(Harness {
		pool,
		deposits,
		ledger,
		relay,
		notify,
	})
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

/// Provision a fresh active user — the withdrawal path's Read-First requires one, and a real
/// row keeps the cross-flow tests faithful to production.
async fn active_user(users: &PgUsers) -> UserId {
	let subject = AuthSubject::parse(&format!("itest-{}", Uuid::new_v4())).unwrap();
	let email = Email::parse(&format!("u{}@example.com", Uuid::new_v4().simple())).unwrap();
	users.provision(subject, email, true).await.unwrap().id()
}

/// The user's cost-basis projection for a fund (None when no `fund_positions` row exists).
async fn cost_basis(positions: &PgFundPositions, user: UserId, service: &ServiceId) -> Option<Usdt> {
	positions.find(user, service).await.unwrap().map(|p| p.cost_basis)
}

/// The projection-tracked remaining units for a fund — the cost-basis reduction's
/// denominator, read straight from the column (the read port does not surface it).
async fn tracked_units(pool: &PgPool, user: UserId, service: &ServiceId) -> Option<Shares> {
	let raw: Option<String> = sqlx::query_scalar("SELECT units FROM fund_positions WHERE user_id = $1 AND service = $2")
		.bind(user.raw())
		.bind(service.as_str())
		.fetch_optional(pool)
		.await
		.unwrap();
	raw.map(|s| Shares::from_base_units(s.parse().unwrap()))
}

/// A structurally valid destination address for a withdrawal (distinct from the user's own).
fn destination(network: Network) -> WalletAddress {
	let raw = match network {
		Network::Bep20 => "0x52908400098527886E0F7030069857D2E4169EE7",
		Network::Trc20 => "TJRabPrwbZy45sbavfcjinPJC18kjpRTv8",
		Network::Ton => "EQCD39VS5jcptHL8vMjEXrzGaRcCVYto7HUn4bpAOg8xqB2N",
	};
	WalletAddress::parse(network, raw).unwrap()
}

#[tokio::test]
async fn deposit_credits_once_and_is_idempotent_by_tx_ref() {
	let Some(h) = harness().await else { return };
	let user = UserId::new();
	let network = Network::Trc20;
	let key = LedgerAccountKey::UserClaim(user);
	let tx_ref = unique_tx_ref();

	assert!(claim(&h, &key).await.is_zero());

	let recorded = balance_app::record_deposit(&h.deposits, &h.notify, tx_ref.clone(), Party::User(user), network, usdt("100"))
		.await
		.unwrap();
	assert!(recorded, "first record is new");
	h.relay.drain().await;
	assert_eq!(claim(&h, &key).await, usdt("100"), "the deposit credited the user's claim");

	// Re-recording the same chain tx is a no-op — no second event, no double credit.
	let again = balance_app::record_deposit(&h.deposits, &h.notify, tx_ref, Party::User(user), network, usdt("100"))
		.await
		.unwrap();
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

	balance_app::record_deposit(&h.deposits, &h.notify, unique_tx_ref(), Party::User(user), network, usdt("250"))
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
	balance_app::record_deposit(&h.deposits, &h.notify, unique_tx_ref(), Party::User(user), network, usdt("10"))
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
	balance_app::record_deposit(&h.deposits, &h.notify, unique_tx_ref(), Party::User(user), network, usdt("100"))
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
	assert_eq!(nav_repo.current(&service).await.unwrap().map(|v| v.nav).unwrap_or(Nav::SEED), Nav::SEED);
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
	assert_eq!(nav_repo.current(&service).await.unwrap().map(|v| v.nav).unwrap_or(Nav::SEED), Nav::parse_decimal("1.5").unwrap());

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

	balance_app::record_deposit(&h.deposits, &h.notify, unique_tx_ref(), Party::User(user), Network::Bep20, usdt("400"))
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

	balance_app::record_deposit(&h.deposits, &h.notify, unique_tx_ref(), Party::User(user), Network::Bep20, usdt("100"))
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
	balance_app::record_deposit(&h.deposits, &h.notify, unique_tx_ref(), Party::User(user), Network::Bep20, usdt("100"))
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
	balance_app::record_deposit(&h.deposits, &h.notify, unique_tx_ref(), Party::Service(service.clone()), Network::Bep20, usdt("100"))
		.await
		.unwrap();
	h.relay.drain().await;
	assert_eq!(claim(&h, &service_claim).await, usdt("200"), "the fund topped up");

	// Operator settles — priced at the settle-time NAV (2) and paid in full.
	let settled = funds_app::settle_redemption(&reds, &nav_repo, &h.notify, id, now).await.unwrap();
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
	funds_app::settle_redemption(&reds, &nav_repo, &h.notify, id, now).await.unwrap();
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

// BANK-MONEY-1: the cost-basis projection is now written by the relay AFTER the cash leg
// posts, never on the synchronous `open` path. So a subscription whose cash leg parks
// (insufficient claim) leaves NO `fund_positions` row — no phantom basis without units/cash.
// The `open` repo is called directly to bypass the application Read-First and force the relay
// to face an over-subscription, the exact race the fix guards.
#[tokio::test]
async fn a_parked_subscribe_cash_leg_leaves_no_cost_basis() {
	let Some(h) = harness().await else { return };
	let subs = PgSubscriptions::new(h.pool.clone());
	let positions = PgFundPositions::new(h.pool.clone());
	let user = UserId::new();
	let service = unique_service();
	let user_claim = LedgerAccountKey::UserClaim(user);

	// Fund only 50, then commit a 100-cash subscription straight through the repo (skipping the
	// application solvency check) so the relay's cash leg `Dr user / Cr service` overdraws.
	balance_app::record_deposit(&h.deposits, &h.notify, unique_tx_ref(), Party::User(user), Network::Bep20, usdt("50"))
		.await
		.unwrap();
	h.relay.drain().await;

	let mut subscription = Subscription::open(SubscriptionId::new(), user, service.clone(), usdt("100"), Nav::SEED).unwrap();
	subs.open(&mut subscription).await.unwrap();
	h.relay.drain().await;

	// TB's flag parked the cash leg: no cash moved, no units minted, and — the fix — no basis.
	assert_eq!(claim(&h, &user_claim).await, usdt("50"), "the cash never left the claim (cash leg parked)");
	assert_eq!(units(&h, &LedgerAccountKey::UserShares(service.clone(), user)).await, Shares::ZERO, "no units minted");
	assert!(cost_basis(&positions, user, &service).await.is_none(), "a parked subscribe must leave no phantom cost_basis");
}

// BANK-ARCH-02: a concurrent withdraw + subscribe on the same UserClaim must serialize on the
// shared per-user lock so the system never ends in a silently-divergent state. With 100
// deposited, an 80-gross withdrawal and an 80-cash subscription cannot both apply (160 > 100):
// the shared lock orders the two `open` commits, and the relay (single-worker, seq-ordered)
// then applies the first reservation while the second hits TB's non-negative flag and PARKS —
// a recoverable terminal, not a silent over-commit. The combined fix guarantees the only
// observable end state is consistent: exactly one 80-spend posts, the claim never goes
// negative, and — the BANK-MONEY-1 half — the subscribe leaves cost_basis ONLY when its cash
// leg actually posted (never a phantom basis behind a parked cash leg).
#[tokio::test]
async fn concurrent_withdraw_and_subscribe_never_leave_a_divergent_claim() {
	let Some(h) = harness().await else { return };
	let users = PgUsers::new(h.pool.clone());
	let subs: Arc<dyn SubscriptionRepository> = Arc::new(PgSubscriptions::new(h.pool.clone()));
	let withdrawals: Arc<dyn piggybank_core::ports::WithdrawalRepository> = Arc::new(PgWithdrawals::new(h.pool.clone()));
	let users_dyn: Arc<dyn UserRepository> = Arc::new(PgUsers::new(h.pool.clone()));
	let nav_repo = PgNav::new(h.pool.clone());
	let positions = PgFundPositions::new(h.pool.clone());
	let user = active_user(&users).await;
	let service = unique_service();
	let now = now_unix();
	let user_claim = LedgerAccountKey::UserClaim(user);
	let service_claim = LedgerAccountKey::ServiceClaim(service.clone());
	let network = Network::Bep20;

	balance_app::record_deposit(&h.deposits, &h.notify, unique_tx_ref(), Party::User(user), network, usdt("100"))
		.await
		.unwrap();
	h.relay.drain().await;

	// Fire both at once; the shared `users` advisory lock inside each `open` serializes the two
	// commits so they can never interleave a half-applied write.
	let sub_fut = funds_app::subscribe(subs.as_ref(), h.ledger.as_ref(), &nav_repo, &h.notify, user, service.clone(), usdt("80"), now);
	let wd_fut = withdrawal_app::request_withdrawal(
		withdrawals.as_ref(),
		h.ledger.as_ref(),
		users_dyn.as_ref(),
		&StubCustody,
		&h.notify,
		&Network::ALL,
		user,
		network,
		destination(network),
		usdt("80"),
	);
	let (sub_res, wd_res) = tokio::join!(sub_fut, wd_fut);

	// At least one must succeed (100 covers a single 80-spend); the relay then applies the
	// serialized reservations and parks the over-commit.
	assert!(sub_res.is_ok() || wd_res.is_ok(), "at least one request must succeed (the claim covers 80)");
	h.relay.drain().await;

	// Exactly one 80-spend landed — the other parked, never half-applied — so the claim shows
	// 20 spendable regardless of which won the relay race, and never goes negative (TB's flag).
	// `cash_in` is the cash a *winning* subscribe moved into the fund (zero if it parked): the
	// user's posted balance dropped only by a cash leg that actually moved (a withdrawal reserve
	// only locks, leaving posted at 100).
	let bal = h.ledger.balance(&user_claim).await.unwrap();
	let cash_in = claim(&h, &service_claim).await;
	assert_eq!(
		Usdt::from_base_units(bal.available()),
		usdt("20"),
		"exactly one 80-spend applied; the over-commit parked, nothing stranded"
	);
	assert_eq!(
		Usdt::from_base_units(bal.posted),
		usdt("100").checked_sub(cash_in).unwrap(),
		"posted dropped only by a cash leg that actually moved"
	);

	// The BANK-MONEY-1 invariant under contention: cost_basis is present IFF the subscribe's
	// cash leg posted (the fund's `service` claim holds the 80). A parked subscribe leaves no
	// phantom basis.
	let basis = cost_basis(&positions, user, &service).await;
	assert_eq!(basis.is_some(), !cash_in.is_zero(), "cost_basis exists iff the subscribe cash leg posted — never a phantom");
	if let Some(b) = basis {
		assert_eq!(b, cash_in, "the recorded basis matches the cash that actually entered the fund");
	}
}

// BANK-MONEY-3: two queued redemptions on ONE position settling back-to-back must reduce the
// average-cost basis by COMPOUNDING fractions, not each dividing by the same pre-burn units.
// The old code read units_held live from TB (`holding.posted`), which lags the async burn, so
// every settle divided by the gross 100 → an under-reduced (inflated) basis. The fix divides
// by the position's own projection-tracked units (decremented under the settle lock). Subscribe
// 100 @ NAV 1 → basis 100, units 100. Mark NAV to 4 so each 30-unit redemption (120 cash > the
// 100 fund claim) QUEUES; top the fund up, then settle both. Sequential-correct:
//   settle 30: basis = trunc(100 × 70/100) = 70, units 70
//   settle 30: basis = trunc(70  × 40/70 ) = 40, units 40
// The relay-lagging bug would compute 70 then trunc(70 × 70/100) = 49 — over-stated by 9.
#[tokio::test]
async fn back_to_back_settles_compound_the_cost_basis_reduction() {
	let Some(h) = harness().await else { return };
	let subs = PgSubscriptions::new(h.pool.clone());
	let reds = PgRedemptions::new(h.pool.clone());
	let nav_repo = PgNav::new(h.pool.clone());
	let positions = PgFundPositions::new(h.pool.clone());
	let user = UserId::new();
	let service = unique_service();
	let now = now_unix();

	balance_app::record_deposit(&h.deposits, &h.notify, unique_tx_ref(), Party::User(user), Network::Bep20, usdt("100"))
		.await
		.unwrap();
	h.relay.drain().await;
	funds_app::subscribe(&subs, h.ledger.as_ref(), &nav_repo, &h.notify, user, service.clone(), usdt("100"), now)
		.await
		.unwrap();
	h.relay.drain().await;
	assert_eq!(cost_basis(&positions, user, &service).await, Some(usdt("100")), "basis seeded by the subscribe");
	assert_eq!(tracked_units(&h.pool, user, &service).await, Some(shares("100")), "units tracked on the projection");

	// Mark NAV to 4 (AUM 400 / 100 units, a forced +300% move) so each 30-unit redemption prices
	// to 120 cash — above the fund's 100 claim — and stays Queued (no auto-settle).
	funds_app::post_fund_valuation(&nav_repo, h.ledger.as_ref(), service.clone(), usdt("400"), "op", true)
		.await
		.unwrap();
	let r1 = funds_app::request_redemption(&reds, h.ledger.as_ref(), &nav_repo, &h.notify, user, service.clone(), shares("30"), now)
		.await
		.unwrap();
	let r2 = funds_app::request_redemption(&reds, h.ledger.as_ref(), &nav_repo, &h.notify, user, service.clone(), shares("30"), now)
		.await
		.unwrap();
	assert_eq!(r1.state(), RedemptionState::Queued, "short fund queues the first redemption");
	assert_eq!(r2.state(), RedemptionState::Queued, "short fund queues the second redemption");
	h.relay.drain().await;

	// Top the fund up so both settles' payouts clear the relay pre-check (2 × 120 = 240).
	balance_app::record_deposit(&h.deposits, &h.notify, unique_tx_ref(), Party::Service(service.clone()), Network::Bep20, usdt("140"))
		.await
		.unwrap();
	h.relay.drain().await;

	// Settle both back-to-back — the under-reduction bug surfaces on the SECOND settle.
	funds_app::settle_redemption(&reds, &nav_repo, &h.notify, r1.id(), now).await.unwrap();
	funds_app::settle_redemption(&reds, &nav_repo, &h.notify, r2.id(), now).await.unwrap();
	h.relay.drain().await;

	assert_eq!(
		cost_basis(&positions, user, &service).await,
		Some(usdt("40")),
		"compounded reduction (100→70→40), not the relay-lagging 49"
	);
	assert_eq!(
		tracked_units(&h.pool, user, &service).await,
		Some(shares("40")),
		"tracked units decremented per settle (100→70→40)"
	);
	assert_eq!(
		units(&h, &LedgerAccountKey::UserShares(service.clone(), user)).await,
		shares("40"),
		"TB holding agrees: 60 units burned"
	);
}
