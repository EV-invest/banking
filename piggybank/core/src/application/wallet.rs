//! Wallet query use cases — a user's per-network position and deposit addresses.
//!
//! Read-First: balances come live from TigerBeetle (the authoritative data plane);
//! the allocated figure is the sum of the user's active stakes (a Postgres
//! projection). Deposit addresses are provisioned lazily through the
//! [`DepositAddresses`] port (stub HD derivation) on first read.

use domain::{
	allocations::{AllocationKind, AllocationState},
	balance::LedgerAccountKey,
	error::DomainError,
	money::{Network, Usdt, WalletAddress},
	users::UserId,
	withdrawals::WithdrawalPolicy,
};

use crate::ports::{AllocationRepository, DepositAddresses, ledger::Ledger};

/// One network's slice of a user's wallet — every figure non-negative.
pub struct WalletNetwork {
	pub network: Network,
	/// Free, spendable now (claim posted − reserved).
	pub available: Usdt,
	/// Locked by in-flight withdrawals (pending claim debits).
	pub reserved: Usdt,
	/// Staked in fund services (sum of the user's active allocations on this network).
	pub allocated: Usdt,
	/// `available + reserved + allocated` — the user's whole position on this network.
	pub total: Usdt,
	/// Where to send USDT on this network (provisioned on first read).
	pub deposit_address: Option<WalletAddress>,
	/// Smallest gross withdrawal accepted on this network.
	pub min_withdrawal: Usdt,
	/// Flat fee retained on a withdrawal on this network.
	pub withdrawal_fee: Usdt,
}

pub struct Wallet {
	pub networks: Vec<WalletNetwork>,
}

/// The caller's wallet across every network: live balances, the allocated total, and
/// a deposit address per network.
pub async fn get_wallet(ledger: &dyn Ledger, allocations: &dyn AllocationRepository, deposit_addresses: &dyn DepositAddresses, user: UserId) -> Result<Wallet, DomainError> {
	let user_allocations = allocations.list_by_user(user).await?;
	let mut networks = Vec::with_capacity(Network::ALL.len());
	for network in Network::ALL {
		let balance = ledger.balance(&LedgerAccountKey::UserClaim(user, network)).await?;
		let available = balance.available();
		let reserved = balance.locked;
		// Allocated = the user's active stakes on this network (revoked/other-kind excluded).
		let allocated = user_allocations
			.iter()
			.filter(|a| a.network() == network && a.state() == AllocationState::Active && matches!(a.kind(), AllocationKind::UserStake { .. }))
			.try_fold(Usdt::ZERO, |acc, a| acc.checked_add(a.amount()))
			.ok_or_else(|| DomainError::Repository("allocated total overflow".into()))?;
		let total = available
			.checked_add(reserved)
			.and_then(|sum| sum.checked_add(allocated))
			.ok_or_else(|| DomainError::Repository("wallet total overflow".into()))?;
		let deposit_address = Some(deposit_addresses.address(user, network).await?);
		networks.push(WalletNetwork {
			network,
			available,
			reserved,
			allocated,
			total,
			deposit_address,
			min_withdrawal: WithdrawalPolicy::minimum(network),
			withdrawal_fee: WithdrawalPolicy::fee(network),
		});
	}
	Ok(Wallet { networks })
}

/// The caller's deposit address on `network` (stable; derived once and reused).
pub async fn get_deposit_address(deposit_addresses: &dyn DepositAddresses, user: UserId, network: Network) -> Result<WalletAddress, DomainError> {
	deposit_addresses.address(user, network).await
}
