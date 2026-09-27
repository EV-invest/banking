//! Integration tests for chain-proven capital seeding (issues #234, #245) — real Postgres
//! **and** TigerBeetle, no mocks of either; they run when `DATABASE_URL` is set and a
//! TigerBeetle replica is reachable (`nix run .#db` + `.#tb`), and skip otherwise. The two
//! gateways (chain reads, address ownership) are separate trust domains and are faked in
//! memory, as every suite here does.
//!
//! A seed is the owners' decision, not an administrator's write: `SeedCapital` opens a
//! `seed_capital` consilium, and the deposit plus the `fund` subscription are booked only
//! when the quorum executes it. So every path here is open → two approvals → execute.
//!
//! What's proven: the seed takes the amount from the chain and never from the caller (a
//! proposal whose amount disagrees with the chain is refused at open); a transfer that
//! landed on a USER's deposit address is refused as capital and leaves nothing behind; an
//! external transfer into the treasury is booked **once**, as the depositor's deposit plus
//! their subscription into the `fund` allocation — the units land with the person, the
//! cash on `service:fund`, and the retired `Fund` claim never moves; executing the same
//! consilium again is an idempotent no-op, the same `tx_ref` cannot be proposed twice,
//! and a second transfer doubles the holding; a re-execution after a crash between the
//! deposit and the subscription opens the missing subscription for the depositor and
//! nobody else; the depositor reads `fund` as a position even though the catalog hides
//! it; an operator's `RecordDeposit` refuses the treasury, pointing at the seed; and an
//! administrator without a seat, or an owner without a quorum, books nothing.
//!
//! Every test here reads the one platform-wide `fund` allocation and the one owner
//! roster, so the suite runs serially under the shared outbox guard — the same rule
//! `ownership_fee` applies — and starts from a cleared governance state.

mod common;

use std::sync::Arc;

use async_trait::async_trait;
use domain::{
	auth::AuthSubject,
	balance::{LedgerAccountKey, ServiceId},
	consilium::{ConsiliumEffect, ConsiliumId, ConsiliumKind, ConsiliumState, ConsiliumTerms, SeedCapitalTerms, VoteDecision},
	error::DomainError,
	money::{Nav, Network, Shares, TxRef, Usdt, WalletAddress},
	users::{Email, UserId},
};
use piggybank_core::{
	application::{balance as balance_app, consilium as consilium_app, funds as funds_app},
	config::KycGate,
	infrastructure::{
		allocations::PgAllocations, consilium::PgConsilia, custody::StubCustody, deposits::PgDeposits, fee_policy_changes::PgFeePolicyChanges, issuance::PgUnitIssuances, nav::PgNav,
		outflow::PgOutflowPolicy, payments::PgPayments, positions::PgFundPositions, relay::Relay, subscriptions::PgSubscriptions, users::PgUsers, withdrawals::PgWithdrawals,
	},
	ports::{
		Custody, CustodyError, SubscriptionRepository, UserRepository,
		consilium::{ConsiliumView, VoteAudit},
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
const CONFIGURED: [Network; 1] = [NETWORK];
const APPROVAL_URL_BASE: &str = "https://example.test/consilium";
const CONSENT_URL_BASE: &str = "https://example.test/consent";

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
	consilia: PgConsilia,
	withdrawals: PgWithdrawals,
	payments: PgPayments,
	outflow: PgOutflowPolicy,
	fee_changes: PgFeePolicyChanges,
	issuances: PgUnitIssuances,
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
	let h = Harness {
		allocations: PgAllocations::new(pool.clone()),
		users: PgUsers::new(pool.clone()),
		subs: PgSubscriptions::new(pool.clone()),
		positions: PgFundPositions::new(pool.clone()),
		nav: PgNav::new(pool.clone()),
		deposits: PgDeposits::new(pool.clone()),
		consilia: PgConsilia::new(pool.clone()),
		withdrawals: PgWithdrawals::new(pool.clone()),
		payments: PgPayments::new(pool.clone()),
		outflow: PgOutflowPolicy::new(pool.clone()),
		fee_changes: PgFeePolicyChanges::new(pool.clone()),
		issuances: PgUnitIssuances::new(pool.clone()),
		relay: Relay::new(pool.clone(), ledger.clone(), Arc::new(StubCustody), notify.clone()),
		ledger,
		notify,
		pool,
		_serial: serial,
	};
	reset_governance(&h).await;
	Some(h)
}

/// Clear the governance state this suite shares: close any open consilium (one may be
/// open per source claim, and every seed takes the fund allocation's slot), empty the
/// owner roster, and forget any roster change so the cooling-off clock cannot freeze a
/// later test. Run at the START of every test, so a panicking test cannot wedge the ones
/// after it.
async fn reset_governance(h: &Harness) {
	sqlx::query("UPDATE consilium SET state = 'cancelled', decided_at = now() WHERE state = 'open'")
		.execute(&h.pool)
		.await
		.unwrap();
	sqlx::query("UPDATE users SET role = 'investor' WHERE role = 'owner'").execute(&h.pool).await.unwrap();
	sqlx::query("DELETE FROM governance_roster_change").execute(&h.pool).await.unwrap();
}

/// The consilium ports over the harness and one faked chain + address view — the same
/// wiring the `SeedCapital` RPC and the sweeper hand the use case.
fn ports<'a>(h: &'a Harness, chain: &'a OneTransferChain, addresses: &'a OneUserAddresses) -> consilium_app::ConsiliumPorts<'a> {
	consilium_app::ConsiliumPorts {
		consilia: &h.consilia,
		withdrawals: &h.withdrawals,
		payments: &h.payments,
		users: &h.users,
		ledger: h.ledger.as_ref(),
		custody: chain,
		policy: &h.outflow,
		allocations: &h.allocations,
		nav: &h.nav,
		fee_changes: &h.fee_changes,
		issuances: &h.issuances,
		deposits: &h.deposits,
		addresses,
		subscriptions: &h.subs,
		relay: &h.notify,
		configured: &CONFIGURED,
		kyc: KycGate::LIFTED,
		approval_url_base: APPROVAL_URL_BASE,
		consent_url_base: CONSENT_URL_BASE,
		governance_mail_wired: true,
	}
}

/// The seed's own ports — the writer a consilium's execution calls, reached directly only
/// to pin the writer's guards (it is `pub` for the execution path and the data migration).
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

fn seed_terms(tx_ref: &TxRef, amount: &str, depositor: UserId) -> SeedCapitalTerms {
	SeedCapitalTerms::new(tx_ref.clone(), NETWORK, usdt(amount), depositor).unwrap()
}

/// A real `users` row — the position projection, the holder read and the seed's open gate
/// name it.
async fn person(h: &Harness) -> UserId {
	let subject = AuthSubject::parse(&format!("seed-{}", Uuid::new_v4())).unwrap();
	let email = Email::parse(&format!("s{}@example.com", Uuid::new_v4().simple())).unwrap();
	h.users.provision(subject, email, true).await.unwrap().id()
}

/// Provision a fresh person and seat them as a fund owner.
async fn owner(h: &Harness) -> UserId {
	let id = person(h).await;
	sqlx::query("UPDATE users SET role = 'owner', concierge_user_id = $2 WHERE id = $1")
		.bind(id.raw())
		.bind(Uuid::new_v4())
		.execute(&h.pool)
		.await
		.unwrap();
	id
}

/// Seat `n` owners. The first is the initiator — and the depositor — in every seed here.
async fn owners(h: &Harness, n: usize) -> Vec<UserId> {
	let mut roster = Vec::with_capacity(n);
	for _ in 0..n {
		roster.push(owner(h).await);
	}
	roster
}

/// The token and code that were mailed to one seat, read out of the queue exactly as the
/// owner reads them off the message.
async fn credentials(h: &Harness, consilium: ConsiliumId, voter: UserId) -> (String, String) {
	let payload: String = sqlx::query_scalar("SELECT payload::text FROM consilium_mail WHERE consilium_id = $1 AND user_id = $2 AND kind = 'payment_approval'")
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
async fn vote(h: &Harness, consilium: ConsiliumId, voter: UserId, decision: VoteDecision) -> bool {
	let (token, code) = credentials(h, consilium, voter).await;
	let audit = VoteAudit {
		client_ip: "203.0.113.7".to_owned(),
		user_agent: "itest".to_owned(),
	};
	consilium_app::submit_decision(&h.consilia, &token, &code, decision, &audit, now_unix()).await.unwrap().decided
}

/// Both peers approve, and the quorum is executed — the whole of a seed after its proposal.
async fn carry(h: &Harness, chain: &OneTransferChain, addresses: &OneUserAddresses, roster: &[UserId], id: ConsiliumId) -> ConsiliumView {
	assert!(!vote(h, id, roster[1], VoteDecision::Approve).await, "one approval of two does not carry");
	assert!(vote(h, id, roster[2], VoteDecision::Approve).await, "the second approval carries the quorum");
	consilium_app::execute(&ports(h, chain, addresses), id, now_unix()).await.unwrap()
}

async fn state_of(h: &Harness, id: ConsiliumId) -> ConsiliumState {
	consilium_app::find(&h.consilia, id).await.unwrap().consilium.state()
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

async fn seed_consilia(pool: &PgPool) -> i64 {
	sqlx::query_scalar("SELECT count(*) FROM consilium WHERE kind = 'seed_capital'").fetch_one(pool).await.unwrap()
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
	let roster = owners(&h, 3).await;
	let tx_ref = unique_tx_ref();
	let before = seed_consilia(&h.pool).await;

	let err = consilium_app::open_seed_capital(&ports(&h, &chain, &addresses), roster[0], seed_terms(&tx_ref, "100", roster[0]), now_unix())
		.await
		.expect_err("a user's deposit must not be proposed as someone's seed");
	assert!(matches!(err, DomainError::Validation(_)), "refused as a validation error, got {err:?}");
	assert!(err.to_string().contains("RecordDeposit"), "the refusal points the operator at the right RPC: {err}");
	assert_eq!(seed_consilia(&h.pool).await, before, "a refused proposal opens no consilium");
	assert_eq!(deposit_row(&h.pool, &tx_ref).await, None, "and leaves no deposit row behind");
	assert_eq!(subscription_rows(&h.pool, roster[0]).await, 0, "and opens no subscription");
}

/// The seed, end to end: proposed, approved by two peers, executed — the treasury arrival
/// is the depositor's deposit and their subscription into `fund`, units with the person,
/// cash on the allocation's claim, the retired `Fund` claim untouched. Executing again is a
/// no-op; the same reference cannot be proposed again; a second reference is a second seed.
#[tokio::test]
async fn an_external_treasury_arrival_seeds_the_depositor_once_per_reference() {
	let Some(h) = harness().await else { return };
	let chain = OneTransferChain {
		transfer: transfer(OUTSIDER, TREASURY, "250.5"),
	};
	let addresses = OneUserAddresses { user: UserId::new() };
	let roster = owners(&h, 3).await;
	let depositor = roster[0];
	let fund = ServiceId::fund();
	let claim_before = cash_of(&h, LedgerAccountKey::ServiceClaim(fund.clone())).await;
	let supply_before = units_of(&h, LedgerAccountKey::SharesOutstanding(fund.clone())).await;
	let retired_before = cash_of(&h, retired_fund_claim()).await;
	// This binary's ledger starts empty, and the tests before this one only ever seed at
	// the price they read, so `fund` is still at par: 250.5 USDT buys 250.5 units.
	let price = funds_app::nav_of(&h.nav, h.ledger.as_ref(), &fund).await.unwrap().nav;
	assert_eq!(price, Nav::SEED, "the fund allocation is priced at par in this binary");

	let tx_ref = unique_tx_ref();
	let opened = consilium_app::open_seed_capital(&ports(&h, &chain, &addresses), depositor, seed_terms(&tx_ref, "250.5", depositor), now_unix())
		.await
		.expect("an external transfer into the treasury is a seed the owners can be asked about");
	let id = opened.consilium.id();
	assert_eq!(opened.consilium.kind(), ConsiliumKind::SeedCapital);
	assert_eq!(
		opened.consilium.source_claim(),
		LedgerAccountKey::ServiceClaim(fund.clone()),
		"a seed takes the fund allocation's slot"
	);
	match opened.consilium.terms() {
		ConsiliumTerms::SeedCapital(terms) => {
			assert_eq!((&terms.tx_ref, terms.network, terms.amount, terms.depositor), (&tx_ref, NETWORK, usdt("250.5"), depositor));
		}
		other => panic!("not a seed: {other:?}"),
	}
	// Proposing books nothing.
	assert_eq!(deposit_row(&h.pool, &tx_ref).await, None, "a proposal is not a deposit");
	assert_eq!(subscription_rows(&h.pool, depositor).await, 0, "nor a subscription");
	// Each peer is mailed an invitation that names the arrival and the person; the
	// initiator, who is also the depositor, gets no seat.
	for peer in &roster[1..] {
		let payload: String = sqlx::query_scalar("SELECT payload::text FROM consilium_mail WHERE consilium_id = $1 AND user_id = $2 AND kind = 'payment_approval'")
			.bind(id.raw())
			.bind(peer.raw())
			.fetch_one(&h.pool)
			.await
			.unwrap();
		let mail: serde_json::Value = serde_json::from_str(&payload).unwrap();
		assert_eq!(mail["source"].as_str().unwrap(), format!("treasury arrival {} on bep20", tx_ref.as_str()));
		assert!(mail["destination"].as_str().unwrap().starts_with(&format!("depositor {depositor} (")), "{}", mail["destination"]);
		assert_eq!(mail["amount"].as_str().unwrap(), "250.5");
		assert!(mail["reason"].as_str().unwrap().starts_with("Seed capital:"), "{}", mail["reason"]);
	}
	let initiator_mails: i64 = sqlx::query_scalar("SELECT count(*) FROM consilium_mail WHERE consilium_id = $1 AND user_id = $2")
		.bind(id.raw())
		.bind(depositor.raw())
		.fetch_one(&h.pool)
		.await
		.unwrap();
	assert_eq!(initiator_mails, 0, "the depositor proposes and does not vote on their own seed");

	let executed = carry(&h, &chain, &addresses, &roster, id).await;
	assert_eq!(executed.consilium.state(), ConsiliumState::Executed);
	let subscription_id = executed.consilium.executed_subscription_id().expect("a seed's effect is the fund subscription");
	assert_eq!(
		subscription_id,
		balance_app::seed_subscription_id(&tx_ref),
		"the effect is the subscription derived from the reference"
	);
	assert_eq!(executed.consilium.effect(), Some(ConsiliumEffect::Subscription(subscription_id)));
	let subscription = h.subs.find_by_id(subscription_id).await.unwrap().expect("the seed opened the depositor's subscription");
	assert_eq!(subscription.user(), depositor);
	assert_eq!(subscription.service(), &fund);
	assert_eq!(subscription.cash(), usdt("250.5"), "the amount is the chain's, which the terms had to match");
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

	// Re-executing — the sweeper's retry, or a redelivered call — is a no-op on every
	// table and account.
	for _ in 0..3 {
		let again = consilium_app::execute(&ports(&h, &chain, &addresses), id, now_unix()).await.unwrap();
		assert_eq!(again.consilium.executed_subscription_id(), Some(subscription_id));
	}
	common::drain_to_quiescence(&h.relay, &h.pool).await;
	assert_eq!(subscription_rows(&h.pool, depositor).await, 1, "one subscription per reference");
	assert_eq!(units_of(&h, LedgerAccountKey::UserShares(fund.clone(), depositor)).await, minted, "the holding is unchanged");

	// The same reference cannot be put to the owners a second time.
	let err = consilium_app::open_seed_capital(&ports(&h, &chain, &addresses), depositor, seed_terms(&tx_ref, "250.5", depositor), now_unix())
		.await
		.expect_err("a booked reference is not a proposal");
	assert!(matches!(err, DomainError::Conflict(_)), "got {err:?}");
	assert!(err.to_string().contains("already been recorded"), "{err}");

	// A second transfer under its own reference is a second seed, through the same quorum.
	let second = consilium_app::open_seed_capital(&ports(&h, &chain, &addresses), depositor, seed_terms(&unique_tx_ref(), "250.5", depositor), now_unix())
		.await
		.expect("another reference is another seed");
	let second = carry(&h, &chain, &addresses, &roster, second.consilium.id()).await;
	assert_eq!(second.consilium.state(), ConsiliumState::Executed);
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
/// against it. Staged here as such — the quorum has carried, and the deposit is recorded by
/// hand under the reference, as the first half of a crashed execution leaves it. The
/// sweeper's re-execution finds the mint missing under its deterministic id and opens it;
/// the deposit stays one; and the writer refuses to open it for anybody but the person the
/// deposit was booked to.
#[tokio::test]
async fn a_repeated_execution_reopens_a_subscription_the_first_attempt_never_opened() {
	let Some(h) = harness().await else { return };
	let chain = OneTransferChain {
		transfer: transfer(OUTSIDER, TREASURY, "120"),
	};
	let addresses = OneUserAddresses { user: UserId::new() };
	let roster = owners(&h, 3).await;
	let (depositor, someone_else) = (roster[0], person(&h).await);
	let fund = ServiceId::fund();
	let tx_ref = unique_tx_ref();
	let claim_before = cash_of(&h, LedgerAccountKey::ServiceClaim(fund.clone())).await;
	let supply_before = units_of(&h, LedgerAccountKey::SharesOutstanding(fund.clone())).await;
	let price = funds_app::nav_of(&h.nav, h.ledger.as_ref(), &fund).await.unwrap().nav;

	let opened = consilium_app::open_seed_capital(&ports(&h, &chain, &addresses), depositor, seed_terms(&tx_ref, "120", depositor), now_unix())
		.await
		.unwrap();
	let id = opened.consilium.id();
	vote(&h, id, roster[1], VoteDecision::Approve).await;
	assert!(vote(&h, id, roster[2], VoteDecision::Approve).await);
	assert_eq!(state_of(&h, id).await, ConsiliumState::Approved);

	// The first half of an execution, as a crash leaves it.
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

	// Not anyone's to repair: the deposit is the depositor's, so the writer mints nobody
	// else for it — pinned on the writer itself, which is what an execution calls.
	let err = balance_app::seed_fund_capital(&seed_ports(&h, &chain, &addresses), someone_else, tx_ref.clone(), NETWORK, Some(usdt("120")), now_unix())
		.await
		.expect_err("a reference recorded under another name is not this person's seed");
	assert!(matches!(err, DomainError::Conflict(_)), "got {err:?}");
	assert_eq!(subscription_rows(&h.pool, someone_else).await, 0, "nothing was opened for them");
	assert_eq!(subscription_rows(&h.pool, depositor).await, 0);

	// The re-execution — the sweeper picking the approval up — repairs the half-done seed.
	let executed = consilium_app::execute(&ports(&h, &chain, &addresses), id, now_unix()).await.unwrap();
	assert_eq!(executed.consilium.state(), ConsiliumState::Executed, "{:?}", executed.consilium.failure_reason());
	let subscription_id = executed.consilium.executed_subscription_id().expect("the subscription the first attempt never opened");
	let subscription = h.subs.find_by_id(subscription_id).await.unwrap().unwrap();
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

	// Now whole, a further execution is the plain idempotent no-op it always was.
	let again = consilium_app::execute(&ports(&h, &chain, &addresses), id, now_unix()).await.unwrap();
	assert_eq!(again.consilium.executed_subscription_id(), Some(subscription_id));
	assert_eq!(subscription_rows(&h.pool, depositor).await, 1);
}

#[tokio::test]
async fn the_sweep_and_a_wrong_expected_amount_are_refused_at_open() {
	let Some(h) = harness().await else { return };
	let addresses = OneUserAddresses { user: UserId::new() };
	let roster = owners(&h, 3).await;
	let depositor = roster[0];
	let before = seed_consilia(&h.pool).await;

	// The sweep: a user's deposit address paying INTO the treasury is money already on the ledger.
	let sweep = OneTransferChain {
		transfer: transfer(USER_ADDRESS, TREASURY, "40"),
	};
	let tx_ref = unique_tx_ref();
	let err = consilium_app::open_seed_capital(&ports(&h, &sweep, &addresses), depositor, seed_terms(&tx_ref, "40", depositor), now_unix())
		.await
		.expect_err("the sweep is not new capital");
	assert!(matches!(err, DomainError::Validation(_)), "got {err:?}");
	assert_eq!(deposit_row(&h.pool, &tx_ref).await, None);

	// A proposal that disagrees with the chain is a refusal, never a vote over either number.
	let chain = OneTransferChain {
		transfer: transfer(OUTSIDER, TREASURY, "100"),
	};
	let tx_ref = unique_tx_ref();
	let err = consilium_app::open_seed_capital(&ports(&h, &chain, &addresses), depositor, seed_terms(&tx_ref, "99", depositor), now_unix())
		.await
		.expect_err("the proposed amount must match the chain");
	assert!(matches!(err, DomainError::Validation(_)), "got {err:?}");
	assert!(err.to_string().contains("the chain reports 100"), "{err}");
	assert_eq!(deposit_row(&h.pool, &tx_ref).await, None);
	assert_eq!(subscription_rows(&h.pool, depositor).await, 0, "a refused proposal opens no subscription");
	assert_eq!(seed_consilia(&h.pool).await, before, "and no consilium");

	// A person nobody can sign in as, or one who is frozen, cannot be seated by a seed.
	let err = consilium_app::open_seed_capital(&ports(&h, &chain, &addresses), depositor, seed_terms(&tx_ref, "100", UserId::new()), now_unix())
		.await
		.expect_err("units for a UUID with no user row are units nobody can redeem");
	assert!(matches!(err, DomainError::NotFound { entity: "user", .. }), "got {err:?}");
	let frozen = person(&h).await;
	sqlx::query("UPDATE users SET status = 'disabled' WHERE id = $1")
		.bind(frozen.raw())
		.execute(&h.pool)
		.await
		.unwrap();
	let err = consilium_app::open_seed_capital(&ports(&h, &chain, &addresses), depositor, seed_terms(&tx_ref, "100", frozen), now_unix())
		.await
		.expect_err("a disabled account is not a holder the owners should be asked to seat");
	assert!(matches!(err, DomainError::Precondition(_)), "got {err:?}");
	assert_eq!(seed_consilia(&h.pool).await, before);
}

/// The finding this suite closes (H-1 of the #245 security review): the chain proves that
/// a dollar reached the treasury, never whose it was, and `SeedCapital` used to let the
/// first administrator to name the reference book it as their own and take `fund` units
/// for it. Now an administrator without a seat cannot even propose it, an owner's
/// proposal books nothing on its own, and an unapproved consilium cannot be executed.
#[tokio::test]
async fn an_admin_cannot_seed_a_treasury_arrival_onto_themselves_without_a_quorum() {
	let Some(h) = harness().await else { return };
	let chain = OneTransferChain {
		transfer: transfer(OUTSIDER, TREASURY, "5000"),
	};
	let addresses = OneUserAddresses { user: UserId::new() };
	let roster = owners(&h, 3).await;
	let fund = ServiceId::fund();
	let tx_ref = unique_tx_ref();
	let claim_before = cash_of(&h, LedgerAccountKey::ServiceClaim(fund.clone())).await;
	let before = seed_consilia(&h.pool).await;

	// An administrator with `CapitalManage` but no seat: refused at the door.
	let admin = person(&h).await;
	let err = consilium_app::open_seed_capital(&ports(&h, &chain, &addresses), admin, seed_terms(&tx_ref, "5000", admin), now_unix())
		.await
		.expect_err("attribution of the platform's money is the owners' call");
	assert!(matches!(err, DomainError::Forbidden(_)), "got {err:?}");
	assert_eq!(seed_consilia(&h.pool).await, before, "no consilium was opened");
	assert_eq!(deposit_row(&h.pool, &tx_ref).await, None, "nothing was booked");
	assert_eq!(subscription_rows(&h.pool, admin).await, 0);

	// An owner proposing the arrival as their own: a proposal, and only that.
	let claimant = roster[0];
	let opened = consilium_app::open_seed_capital(&ports(&h, &chain, &addresses), claimant, seed_terms(&tx_ref, "5000", claimant), now_unix())
		.await
		.unwrap();
	let id = opened.consilium.id();
	assert_eq!(opened.consilium.state(), ConsiliumState::Open);
	assert_eq!(deposit_row(&h.pool, &tx_ref).await, None, "proposing books no deposit");
	assert_eq!(subscription_rows(&h.pool, claimant).await, 0, "and mints no units");

	// Their own approval does not exist: the initiator has no seat, and executing an open
	// consilium is refused outright.
	let err = consilium_app::execute(&ports(&h, &chain, &addresses), id, now_unix()).await.unwrap_err();
	assert!(matches!(err, DomainError::Conflict(_)), "got {err:?}");
	// One peer's approval is not a quorum either.
	assert!(!vote(&h, id, roster[1], VoteDecision::Approve).await);
	assert_eq!(state_of(&h, id).await, ConsiliumState::Open);
	let err = consilium_app::execute(&ports(&h, &chain, &addresses), id, now_unix()).await.unwrap_err();
	assert!(matches!(err, DomainError::Conflict(_)), "got {err:?}");
	common::drain_to_quiescence(&h.relay, &h.pool).await;
	assert_eq!(deposit_row(&h.pool, &tx_ref).await, None, "still nothing booked");
	assert_eq!(subscription_rows(&h.pool, claimant).await, 0);
	assert_eq!(units_of(&h, LedgerAccountKey::UserShares(fund.clone(), claimant)).await, Shares::ZERO);
	assert_eq!(
		cash_of(&h, LedgerAccountKey::ServiceClaim(fund.clone())).await,
		claim_before,
		"the fund allocation's claim did not move"
	);

	// And a rejection by the other peer closes it for good: the arrival stays unattributed.
	assert!(vote(&h, id, roster[2], VoteDecision::Reject).await);
	assert_eq!(state_of(&h, id).await, ConsiliumState::Rejected);
	let err = consilium_app::execute(&ports(&h, &chain, &addresses), id, now_unix()).await.unwrap_err();
	assert!(matches!(err, DomainError::Conflict(_)), "got {err:?}");
	assert_eq!(deposit_row(&h.pool, &tx_ref).await, None);
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
