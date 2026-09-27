//! Integration tests for chain-proven capital seeding (issues #234, #245) — real Postgres
//! **and** TigerBeetle, no mocks of either; they run when `DATABASE_URL` is set and a
//! TigerBeetle replica is reachable (`nix run .#db` + `.#tb`), and skip otherwise. The two
//! gateways (chain reads, address ownership) are separate trust domains and are faked in
//! memory, as every suite here does.
//!
//! What's proven: `seed_fund_capital` takes the amount from the chain and never from the
//! caller; a transfer that landed on a USER's deposit address is refused as capital and
//! leaves nothing behind; an external transfer into the treasury is booked **once**, as
//! the depositor's deposit plus their subscription into the `fund` allocation — the units
//! land with the person, the cash on `service:fund`, and the retired `Fund` claim never
//! moves; a repeat of the same `tx_ref` is an idempotent no-op while a second transfer
//! doubles the holding; a repeat after a crash between the deposit and the subscription
//! opens the missing subscription for the depositor and nobody else; the depositor reads
//! `fund` as a position even though the catalog hides it; and an operator's
//! `RecordDeposit` refuses the treasury, pointing at the seed.
//!
//! Every test here reads the one platform-wide `fund` allocation, so the suite runs
//! serially under the shared outbox guard — the same rule `ownership_fee` applies.

mod common;

use std::sync::Arc;

use async_trait::async_trait;
use domain::{
	auth::AuthSubject,
	balance::{LedgerAccountKey, ServiceId},
	error::DomainError,
	money::{Nav, Network, Shares, TxRef, Usdt, WalletAddress},
	users::{Email, UserId},
};
use piggybank_core::{
	application::{balance as balance_app, funds as funds_app},
	infrastructure::{
		allocations::PgAllocations, custody::StubCustody, deposits::PgDeposits, nav::PgNav, positions::PgFundPositions, relay::Relay, subscriptions::PgSubscriptions, users::PgUsers,
	},
	ports::{
		Custody, CustodyError, UserRepository,
		custody::{BroadcastRequest, InboundTransfer, TreasuryFunding},
		deposit_addresses::DepositAddresses,
		ledger::Ledger,
	},
};
use sqlx::PgPool;
use tokio::sync::{MutexGuard, Notify};
use uuid::Uuid;

const NETWORK: Network = Network::Bep20;
const TREASURY: &str = "0x00000000000000000000000000000000000000aa";
const USER_ADDRESS: &str = "0x00000000000000000000000000000000000000bb";
const OUTSIDER: &str = "0x00000000000000000000000000000000000000cc";

/// The chain as the test sees it: one transfer, whatever the reference.
struct OneTransferChain {
	transfer: InboundTransfer,
}

impl domain::architecture::Gateway for OneTransferChain {}

#[async_trait]
impl Custody for OneTransferChain {
	async fn broadcast(&self, _request: &BroadcastRequest) -> Result<(), CustodyError> {
		Ok(())
	}

	async fn inbound_transfer(&self, _network: Network, _tx_ref: &TxRef) -> Result<Option<InboundTransfer>, CustodyError> {
		Ok(Some(self.transfer.clone()))
	}

	async fn treasury_funding(&self, _network: Network) -> Result<Option<TreasuryFunding>, CustodyError> {
		Ok(Some(TreasuryFunding {
			address: TREASURY.to_owned(),
			onchain_usdt: None,
			onchain_gas: None,
			gas_station_address: None,
			gas_station_gas: None,
		}))
	}
}

/// One user owns `USER_ADDRESS`; nothing else is ours.
struct OneUserAddresses {
	user: UserId,
}

impl domain::architecture::Gateway for OneUserAddresses {}

#[async_trait]
impl DepositAddresses for OneUserAddresses {
	async fn address(&self, _user: UserId, network: Network) -> Result<Option<WalletAddress>, DomainError> {
		Ok(Some(WalletAddress::parse(network, USER_ADDRESS)?))
	}

	async fn owner_of(&self, _network: Network, address: &str) -> Result<Option<UserId>, DomainError> {
		Ok(address.eq_ignore_ascii_case(USER_ADDRESS).then_some(self.user))
	}
}

struct Harness {
	pool: PgPool,
	allocations: PgAllocations,
	users: PgUsers,
	subs: PgSubscriptions,
	positions: PgFundPositions,
	nav: PgNav,
	deposits: PgDeposits,
	ledger: Arc<dyn Ledger>,
	relay: Relay,
	notify: Arc<Notify>,
	/// Held for the test's whole life — declared last so it is released after the relay.
	_serial: MutexGuard<'static, ()>,
}

async fn harness() -> Option<Harness> {
	let serial = common::outbox_serial().await;
	let pool = common::pool().await?;
	let ledger = common::seeded_ledger(&pool, "seed capital test").await?;
	let notify = Arc::new(Notify::new());
	Some(Harness {
		allocations: PgAllocations::new(pool.clone()),
		users: PgUsers::new(pool.clone()),
		subs: PgSubscriptions::new(pool.clone()),
		positions: PgFundPositions::new(pool.clone()),
		nav: PgNav::new(pool.clone()),
		deposits: PgDeposits::new(pool.clone()),
		relay: Relay::new(pool.clone(), ledger.clone(), Arc::new(StubCustody), notify.clone()),
		ledger,
		notify,
		pool,
		_serial: serial,
	})
}

/// The seed's ports over the harness and one faked chain + address view.
fn seed_ports<'a>(h: &'a Harness, chain: &'a OneTransferChain, addresses: &'a OneUserAddresses) -> balance_app::SeedPorts<'a> {
	balance_app::SeedPorts {
		deposits: &h.deposits,
		custody: chain,
		addresses,
		subscriptions: &h.subs,
		funds: funds_app::FundPorts {
			allocations: &h.allocations,
			ledger: h.ledger.as_ref(),
			nav: &h.nav,
			relay: &h.notify,
		},
	}
}

fn usdt(decimal: &str) -> Usdt {
	Usdt::parse_decimal(decimal).unwrap()
}

fn unique_tx_ref() -> TxRef {
	TxRef::parse(&format!("itest-{}", Uuid::new_v4())).unwrap()
}

fn now_unix() -> i64 {
	std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

fn transfer(from: &str, to: &str, amount: &str) -> InboundTransfer {
	InboundTransfer {
		from: from.to_owned(),
		to: to.to_owned(),
		amount: usdt(amount),
	}
}

/// A real `users` row — the position projection and the holder read name it.
async fn person(h: &Harness) -> UserId {
	let subject = AuthSubject::parse(&format!("seed-{}", Uuid::new_v4())).unwrap();
	let email = Email::parse(&format!("s{}@example.com", Uuid::new_v4().simple())).unwrap();
	h.users.provision(subject, email, true).await.unwrap().id()
}

async fn deposit_row(pool: &PgPool, tx_ref: &TxRef) -> Option<(String, String, String)> {
	sqlx::query_as::<_, (String, String, String)>("SELECT party_kind, party_id, amount FROM deposits WHERE tx_ref = $1")
		.bind(tx_ref.as_str())
		.fetch_optional(pool)
		.await
		.expect("read deposit row")
}

async fn subscription_rows(pool: &PgPool, user: UserId) -> i64 {
	sqlx::query_scalar("SELECT count(*) FROM subscriptions WHERE user_id = $1 AND service = 'fund'")
		.bind(user.raw())
		.fetch_one(pool)
		.await
		.expect("count subscription rows")
}

async fn cash_of(h: &Harness, key: LedgerAccountKey) -> Usdt {
	Usdt::from_base_units(h.ledger.balance(&key).await.unwrap().posted)
}

async fn units_of(h: &Harness, key: LedgerAccountKey) -> Shares {
	Shares::from_base_units(h.ledger.balance(&key).await.unwrap().posted)
}

/// The retired singleton the seed used to credit. Read through the deprecated key on
/// purpose: the assertion is that it never moves again.
#[allow(deprecated)]
fn retired_fund_claim() -> LedgerAccountKey {
	LedgerAccountKey::Fund
}

#[tokio::test]
async fn a_transfer_to_a_user_address_is_not_seed_capital() {
	let Some(h) = harness().await else { return };
	let chain = OneTransferChain {
		transfer: transfer(OUTSIDER, USER_ADDRESS, "100"),
	};
	let addresses = OneUserAddresses { user: UserId::new() };
	let depositor = person(&h).await;
	let tx_ref = unique_tx_ref();

	let err = balance_app::seed_fund_capital(&seed_ports(&h, &chain, &addresses), depositor, tx_ref.clone(), NETWORK, None, now_unix())
		.await
		.expect_err("a user's deposit must not be booked as someone's seed");
	assert!(matches!(err, DomainError::Validation(_)), "refused as a validation error, got {err:?}");
	assert!(err.to_string().contains("RecordDeposit"), "the refusal points the operator at the right RPC: {err}");
	assert_eq!(deposit_row(&h.pool, &tx_ref).await, None, "a refused seed leaves no deposit row behind");
	assert_eq!(subscription_rows(&h.pool, depositor).await, 0, "and opens no subscription");
}

/// The seed, end to end: the treasury arrival is the depositor's deposit and their
/// subscription into `fund` — units with the person, cash on the allocation's claim, the
/// retired `Fund` claim untouched. The same reference again is a no-op; a second
/// reference doubles the holding.
#[tokio::test]
async fn an_external_treasury_arrival_seeds_the_depositor_once_per_reference() {
	let Some(h) = harness().await else { return };
	let chain = OneTransferChain {
		transfer: transfer(OUTSIDER, TREASURY, "250.5"),
	};
	let addresses = OneUserAddresses { user: UserId::new() };
	let depositor = person(&h).await;
	let fund = ServiceId::fund();
	let claim_before = cash_of(&h, LedgerAccountKey::ServiceClaim(fund.clone())).await;
	let supply_before = units_of(&h, LedgerAccountKey::SharesOutstanding(fund.clone())).await;
	let retired_before = cash_of(&h, retired_fund_claim()).await;
	// This binary's ledger starts empty, and the tests before this one only ever seed at
	// the price they read, so `fund` is still at par: 250.5 USDT buys 250.5 units.
	let price = funds_app::nav_of(&h.nav, h.ledger.as_ref(), &fund).await.unwrap().nav;
	assert_eq!(price, Nav::SEED, "the fund allocation is priced at par in this binary");

	let tx_ref = unique_tx_ref();
	let first = balance_app::seed_fund_capital(&seed_ports(&h, &chain, &addresses), depositor, tx_ref.clone(), NETWORK, None, now_unix())
		.await
		.expect("an external transfer into the treasury seeds the depositor");
	assert!(first.recorded, "the first sighting is recorded");
	assert_eq!(first.amount, usdt("250.5"), "the amount is the chain's, not the caller's");
	let subscription = first.subscription.expect("the seed opened the depositor's subscription");
	assert_eq!(subscription.user(), depositor);
	assert_eq!(subscription.service(), &fund);
	assert_eq!(subscription.cash(), usdt("250.5"));
	assert_eq!(
		deposit_row(&h.pool, &tx_ref).await,
		Some(("user".to_owned(), depositor.to_string(), usdt("250.5").base_units().to_string())),
		"booked as the depositor's deposit, never as the fund's"
	);
	common::drain_to_quiescence(&h.relay, &h.pool).await;

	let minted = Shares::from_cash(usdt("250.5"), Nav::SEED).unwrap();
	assert_eq!(units_of(&h, LedgerAccountKey::UserShares(fund.clone(), depositor)).await, minted, "the units are the depositor's");
	assert_eq!(
		units_of(&h, LedgerAccountKey::SharesOutstanding(fund.clone())).await,
		supply_before.checked_add(minted).unwrap(),
		"the supply grew by exactly the mint"
	);
	assert_eq!(
		cash_of(&h, LedgerAccountKey::ServiceClaim(fund.clone())).await,
		claim_before.checked_add(usdt("250.5")).unwrap(),
		"the cash sits on the fund allocation's claim"
	);
	assert_eq!(
		cash_of(&h, LedgerAccountKey::UserClaim(depositor)).await,
		Usdt::ZERO,
		"nothing is left on the depositor's own claim"
	);
	assert_eq!(cash_of(&h, retired_fund_claim()).await, retired_before, "the retired `Fund` claim does not move");
	assert_eq!(
		funds_app::nav_of(&h.nav, h.ledger.as_ref(), &fund).await.unwrap().nav,
		Nav::SEED,
		"cash in equals units out, so the price stays at par"
	);

	// The same reference again: a no-op on every table and account.
	let again = balance_app::seed_fund_capital(&seed_ports(&h, &chain, &addresses), depositor, tx_ref.clone(), NETWORK, Some(usdt("250.5")), now_unix())
		.await
		.expect("a repeat of the same reference is an idempotent no-op");
	assert!(!again.recorded, "the same tx_ref is never credited twice");
	assert_eq!(again.amount, usdt("250.5"));
	assert!(again.subscription.is_none(), "a repeat mints nothing");
	common::drain_to_quiescence(&h.relay, &h.pool).await;
	assert_eq!(subscription_rows(&h.pool, depositor).await, 1, "one subscription per reference");
	assert_eq!(units_of(&h, LedgerAccountKey::UserShares(fund.clone(), depositor)).await, minted, "the holding is unchanged");

	// A second transfer under its own reference is a second seed.
	let second = balance_app::seed_fund_capital(&seed_ports(&h, &chain, &addresses), depositor, unique_tx_ref(), NETWORK, None, now_unix())
		.await
		.expect("another reference is another seed");
	assert!(second.recorded);
	common::drain_to_quiescence(&h.relay, &h.pool).await;
	assert_eq!(subscription_rows(&h.pool, depositor).await, 2);
	assert_eq!(
		units_of(&h, LedgerAccountKey::UserShares(fund.clone(), depositor)).await,
		minted.checked_add(minted).unwrap(),
		"two transfers, twice the units"
	);
	assert_eq!(cash_of(&h, retired_fund_claim()).await, retired_before, "still nothing on the retired claim");

	// The depositor sees what they hold: `fund` is hidden from the catalog, and a holder
	// reads it anyway, by title and at its price.
	let record = funds_app::allocation_for_holder(&h.allocations, h.ledger.as_ref(), &fund, depositor, false)
		.await
		.expect("a holder reads the hidden allocation");
	assert_eq!(record.allocation.service(), &fund);
	assert_eq!(record.allocation.title(), "Fund allocation");
	let position = funds_app::list_positions(&h.positions, h.ledger.as_ref(), &h.nav, depositor)
		.await
		.unwrap()
		.into_iter()
		.find(|position| position.service == fund)
		.expect("the seed is a position of the depositor's");
	assert_eq!(position.units, minted.checked_add(minted).unwrap());
	assert_eq!(position.value, usdt("501"), "worth what was put in, at par");
}

/// The seed is two commits, and a process can die between them: the deposit recorded, the
/// subscription never opened, the cash resting on the depositor's own claim with no units
/// against it. That is exactly the state a deposit recorded by hand under the reference
/// leaves — staged here as such, not by deleting a row after a full seed, which would
/// leave the mint already on the ledger. A repeat of the seed finds the mint missing
/// under its deterministic id and opens it; the deposit stays one; a repeat under another
/// person's name is refused rather than minted to them.
#[tokio::test]
async fn a_repeated_seed_reopens_a_subscription_the_first_call_never_opened() {
	let Some(h) = harness().await else { return };
	let chain = OneTransferChain {
		transfer: transfer(OUTSIDER, TREASURY, "120"),
	};
	let addresses = OneUserAddresses { user: UserId::new() };
	let (depositor, someone_else) = (person(&h).await, person(&h).await);
	let fund = ServiceId::fund();
	let tx_ref = unique_tx_ref();
	let claim_before = cash_of(&h, LedgerAccountKey::ServiceClaim(fund.clone())).await;
	let supply_before = units_of(&h, LedgerAccountKey::SharesOutstanding(fund.clone())).await;
	let price = funds_app::nav_of(&h.nav, h.ledger.as_ref(), &fund).await.unwrap().nav;

	// The first half of a seed, as a crash leaves it.
	assert!(
		balance_app::record_deposit(&h.deposits, &h.notify, tx_ref.clone(), domain::balance::Party::User(depositor), NETWORK, usdt("120"))
			.await
			.unwrap()
	);
	common::drain_to_quiescence(&h.relay, &h.pool).await;
	assert_eq!(
		cash_of(&h, LedgerAccountKey::UserClaim(depositor)).await,
		usdt("120"),
		"the cash rests on the depositor's own claim"
	);
	assert_eq!(subscription_rows(&h.pool, depositor).await, 0, "and no units stand against it");

	// Not anyone's to repair: the deposit is the depositor's, so nobody else is minted for it.
	let err = balance_app::seed_fund_capital(&seed_ports(&h, &chain, &addresses), someone_else, tx_ref.clone(), NETWORK, None, now_unix())
		.await
		.expect_err("a reference recorded under another name is not this person's seed");
	assert!(matches!(err, DomainError::Conflict(_)), "got {err:?}");
	assert_eq!(subscription_rows(&h.pool, someone_else).await, 0, "nothing was opened for them");
	assert_eq!(subscription_rows(&h.pool, depositor).await, 0);

	let repeat = balance_app::seed_fund_capital(&seed_ports(&h, &chain, &addresses), depositor, tx_ref.clone(), NETWORK, None, now_unix())
		.await
		.expect("the repeat repairs the half-done seed");
	assert!(!repeat.recorded, "the deposit is not new");
	let subscription = repeat.subscription.expect("the subscription the first call never opened");
	assert_eq!(subscription.user(), depositor);
	assert_eq!(subscription.cash(), usdt("120"));
	assert_eq!(subscription.nav(), price, "priced at the fund's NAV as of now");
	common::drain_to_quiescence(&h.relay, &h.pool).await;

	let minted = Shares::from_cash(usdt("120"), price).unwrap();
	assert_eq!(subscription_rows(&h.pool, depositor).await, 1, "one subscription");
	assert_eq!(
		deposit_row(&h.pool, &tx_ref).await,
		Some(("user".to_owned(), depositor.to_string(), usdt("120").base_units().to_string())),
		"and still one deposit, the depositor's"
	);
	assert_eq!(units_of(&h, LedgerAccountKey::UserShares(fund.clone(), depositor)).await, minted, "the units are the depositor's");
	assert_eq!(units_of(&h, LedgerAccountKey::SharesOutstanding(fund.clone())).await, supply_before.checked_add(minted).unwrap());
	assert_eq!(cash_of(&h, LedgerAccountKey::UserClaim(depositor)).await, Usdt::ZERO, "the cash moved off the depositor's claim");
	assert_eq!(
		cash_of(&h, LedgerAccountKey::ServiceClaim(fund.clone())).await,
		claim_before.checked_add(usdt("120")).unwrap(),
		"onto the fund allocation's"
	);

	// Now whole, the reference is the plain idempotent no-op it always was.
	let again = balance_app::seed_fund_capital(&seed_ports(&h, &chain, &addresses), depositor, tx_ref.clone(), NETWORK, None, now_unix())
		.await
		.expect("a repeat of a whole seed is a no-op");
	assert!(!again.recorded);
	assert!(again.subscription.is_none(), "nothing to repair, nothing minted");
	assert_eq!(subscription_rows(&h.pool, depositor).await, 1);
}

#[tokio::test]
async fn the_sweep_and_a_wrong_expected_amount_are_refused() {
	let Some(h) = harness().await else { return };
	let addresses = OneUserAddresses { user: UserId::new() };
	let depositor = person(&h).await;

	// The sweep: a user's deposit address paying INTO the treasury is money already on the ledger.
	let sweep = OneTransferChain {
		transfer: transfer(USER_ADDRESS, TREASURY, "40"),
	};
	let tx_ref = unique_tx_ref();
	let err = balance_app::seed_fund_capital(&seed_ports(&h, &sweep, &addresses), depositor, tx_ref.clone(), NETWORK, None, now_unix())
		.await
		.expect_err("the sweep is not new capital");
	assert!(matches!(err, DomainError::Validation(_)), "got {err:?}");
	assert_eq!(deposit_row(&h.pool, &tx_ref).await, None);

	// An assertion that disagrees with the chain is a refusal, never a credit of either number.
	let chain = OneTransferChain {
		transfer: transfer(OUTSIDER, TREASURY, "100"),
	};
	let tx_ref = unique_tx_ref();
	let err = balance_app::seed_fund_capital(&seed_ports(&h, &chain, &addresses), depositor, tx_ref.clone(), NETWORK, Some(usdt("99")), now_unix())
		.await
		.expect_err("expected_amount must match the chain");
	assert!(matches!(err, DomainError::Validation(_)), "got {err:?}");
	assert_eq!(deposit_row(&h.pool, &tx_ref).await, None);
	assert_eq!(subscription_rows(&h.pool, depositor).await, 0, "a refused seed opens no subscription");
}

/// The general hand-recording path credits only whom the chain names: a treasury arrival
/// names a wallet, not a person, so it is refused there and sent to the seed — never
/// booked against a claim with no holder.
#[tokio::test]
async fn record_deposit_refuses_the_treasury_and_points_at_the_seed() {
	let Some(h) = harness().await else { return };
	let chain = OneTransferChain {
		transfer: transfer(OUTSIDER, TREASURY, "75"),
	};
	let addresses = OneUserAddresses { user: UserId::new() };
	let tx_ref = unique_tx_ref();

	let err = balance_app::record_verified_arrival(&h.deposits, &chain, &addresses, &h.notify, tx_ref.clone(), NETWORK, None)
		.await
		.expect_err("a treasury arrival has no chain-named owner to credit");
	assert!(matches!(err, DomainError::Validation(_)), "got {err:?}");
	assert!(err.to_string().contains("SeedCapital"), "the refusal names the remedy: {err}");
	assert_eq!(deposit_row(&h.pool, &tx_ref).await, None, "nothing was recorded");

	// The same path still credits a user's own address, as before.
	let owner = person(&h).await;
	let to_user = OneTransferChain {
		transfer: transfer(OUTSIDER, USER_ADDRESS, "75"),
	};
	let addresses = OneUserAddresses { user: owner };
	let arrival = balance_app::record_verified_arrival(&h.deposits, &to_user, &addresses, &h.notify, tx_ref.clone(), NETWORK, None)
		.await
		.expect("a transfer to a user's address is that user's deposit");
	assert!(arrival.recorded);
	assert_eq!(arrival.user, owner);
	assert_eq!(
		deposit_row(&h.pool, &tx_ref).await,
		Some(("user".to_owned(), owner.to_string(), usdt("75").base_units().to_string()))
	);
	common::drain_to_quiescence(&h.relay, &h.pool).await;
}
