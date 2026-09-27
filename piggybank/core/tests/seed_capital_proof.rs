//! Integration test for chain-proven capital seeding (issue #234) — real Postgres for
//! the deposit gate, no mocks of it; runs when `DATABASE_URL` is set and skips otherwise.
//! The two gateways (chain reads, address ownership) are separate trust domains and are
//! faked in memory, as every suite here does.
//!
//! What's proven: `seed_fund_capital` takes the amount from the chain and never from the
//! caller; a transfer that landed on a USER's deposit address is refused as capital and
//! leaves no deposit row behind; an external transfer into the treasury is booked once as
//! `Party::Piggybank`, and a second call with the same `tx_ref` is an idempotent no-op.

mod common;

use std::sync::Arc;

use async_trait::async_trait;
use domain::{
	balance::Party,
	error::DomainError,
	money::{Network, TxRef, Usdt, WalletAddress},
	users::UserId,
};
use piggybank_core::{
	application::balance as balance_app,
	infrastructure::deposits::PgDeposits,
	ports::{
		Custody, CustodyError,
		custody::{BroadcastRequest, InboundTransfer, TreasuryFunding},
		deposit_addresses::DepositAddresses,
	},
};
use sqlx::PgPool;
use tokio::sync::Notify;
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

fn usdt(decimal: &str) -> Usdt {
	Usdt::parse_decimal(decimal).unwrap()
}

fn unique_tx_ref() -> TxRef {
	TxRef::parse(&format!("itest-{}", Uuid::new_v4())).unwrap()
}

fn transfer(from: &str, to: &str, amount: &str) -> InboundTransfer {
	InboundTransfer {
		from: from.to_owned(),
		to: to.to_owned(),
		amount: usdt(amount),
	}
}

async fn deposit_row(pool: &PgPool, tx_ref: &TxRef) -> Option<(String, String)> {
	sqlx::query_as::<_, (String, String)>("SELECT party_kind, amount FROM deposits WHERE tx_ref = $1")
		.bind(tx_ref.as_str())
		.fetch_optional(pool)
		.await
		.expect("read deposit row")
}

#[tokio::test]
async fn a_transfer_to_a_user_address_is_not_capital() {
	let Some(pool) = common::pool().await else { return };
	let deposits = PgDeposits::new(pool.clone());
	let notify = Arc::new(Notify::new());
	let chain = OneTransferChain {
		transfer: transfer(OUTSIDER, USER_ADDRESS, "100"),
	};
	let addresses = OneUserAddresses { user: UserId::new() };
	let tx_ref = unique_tx_ref();

	let err = balance_app::seed_fund_capital(&deposits, &chain, &addresses, &notify, tx_ref.clone(), NETWORK, None)
		.await
		.expect_err("a user's deposit must not be booked as fund capital");
	assert!(matches!(err, DomainError::Validation(_)), "refused as a validation error, got {err:?}");
	assert!(err.to_string().contains("RecordDeposit"), "the refusal points the operator at the right RPC: {err}");
	assert_eq!(deposit_row(&pool, &tx_ref).await, None, "a refused seed leaves no deposit row behind");
}

#[tokio::test]
// Drives the retired fund/fee parties on purpose: this flow moves in a later #245 step.
#[allow(deprecated)]
async fn an_external_treasury_arrival_is_capital_once() {
	let Some(pool) = common::pool().await else { return };
	let deposits = PgDeposits::new(pool.clone());
	let notify = Arc::new(Notify::new());
	let chain = OneTransferChain {
		transfer: transfer(OUTSIDER, TREASURY, "250.5"),
	};
	let addresses = OneUserAddresses { user: UserId::new() };
	let tx_ref = unique_tx_ref();

	let first = balance_app::seed_fund_capital(&deposits, &chain, &addresses, &notify, tx_ref.clone(), NETWORK, None)
		.await
		.expect("an external transfer into the treasury is fund capital");
	assert!(first.recorded, "the first sighting is recorded");
	assert_eq!(first.party, Party::Piggybank);
	assert_eq!(first.amount, usdt("250.5"), "the amount is the chain's, not the caller's");
	assert_eq!(
		deposit_row(&pool, &tx_ref).await,
		Some(("piggybank".to_owned(), usdt("250.5").base_units().to_string())),
		"booked against the fund's own claim"
	);

	let again = balance_app::seed_fund_capital(&deposits, &chain, &addresses, &notify, tx_ref.clone(), NETWORK, Some(usdt("250.5")))
		.await
		.expect("a repeat of the same reference is an idempotent no-op");
	assert!(!again.recorded, "the same tx_ref is never credited twice");
	assert_eq!(again.amount, usdt("250.5"));
}

#[tokio::test]
async fn the_sweep_and_a_wrong_expected_amount_are_refused() {
	let Some(pool) = common::pool().await else { return };
	let deposits = PgDeposits::new(pool.clone());
	let notify = Arc::new(Notify::new());
	let addresses = OneUserAddresses { user: UserId::new() };

	// The sweep: a user's deposit address paying INTO the treasury is money already on the ledger.
	let sweep = OneTransferChain {
		transfer: transfer(USER_ADDRESS, TREASURY, "40"),
	};
	let tx_ref = unique_tx_ref();
	let err = balance_app::seed_fund_capital(&deposits, &sweep, &addresses, &notify, tx_ref.clone(), NETWORK, None)
		.await
		.expect_err("the sweep is not new capital");
	assert!(matches!(err, DomainError::Validation(_)), "got {err:?}");
	assert_eq!(deposit_row(&pool, &tx_ref).await, None);

	// An assertion that disagrees with the chain is a refusal, never a credit of either number.
	let chain = OneTransferChain {
		transfer: transfer(OUTSIDER, TREASURY, "100"),
	};
	let tx_ref = unique_tx_ref();
	let err = balance_app::seed_fund_capital(&deposits, &chain, &addresses, &notify, tx_ref.clone(), NETWORK, Some(usdt("99")))
		.await
		.expect_err("expected_amount must match the chain");
	assert!(matches!(err, DomainError::Validation(_)), "got {err:?}");
	assert_eq!(deposit_row(&pool, &tx_ref).await, None);
}
