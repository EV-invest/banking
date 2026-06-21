//! Wallet query use cases — a user's unified balance, deposit rails, and per-rail
//! withdrawal options.
//!
//! The user has **one** network-agnostic claim; the wallet presents it segmented by
//! lifecycle. Read-First: `available` comes live from TigerBeetle (the authoritative
//! data plane); `invested` is the sum of active stakes and `pending_withdrawal` the sum
//! of queued/in-flight withdrawals (Postgres projections). Network re-enters only as a
//! transaction attribute: a per-rail deposit address and a per-rail withdrawable view
//! (`instant = min(available, rail liquidity)`, the accept-and-queue degradation hint).

use domain::{
	allocations::{AllocationKind, AllocationState},
	balance::LedgerAccountKey,
	error::DomainError,
	money::{Network, Usdt, WalletAddress},
	users::UserId,
	withdrawals::{WithdrawalPolicy, WithdrawalState},
};

use crate::ports::{AllocationRepository, DepositAddresses, WithdrawalRepository, ledger::Ledger};

/// A user's single, network-agnostic balance, segmented by lifecycle. Every figure is
/// non-negative; `total = available + invested + pending_withdrawal`.
pub struct WalletBalance {
	/// Free, spendable now (claim posted − reserved).
	pub available: Usdt,
	/// Staked in fund services (sum of the user's active stakes).
	pub invested: Usdt,
	/// Locked by queued/in-flight withdrawals (sum of their gross).
	pub pending_withdrawal: Usdt,
	/// `available + invested + pending_withdrawal` — the user's whole position.
	pub total: Usdt,
}

/// A deposit rail — where to send USDT (on a given chain) to top up the unified balance.
pub struct DepositRail {
	pub network: Network,
	pub address: Option<WalletAddress>,
}

/// Per-rail withdrawal options (the accept-and-queue UX). `withdrawable` is the user's
/// whole available balance (a request beyond `instant` is accepted and queued until the
/// rail is topped up); `instant` is the portion that ships without queueing —
/// `min(available, rail liquidity)`. Note `instant` equals the rail's liquidity exactly
/// when the user's available exceeds it, so it discloses that rail's liquidity up to the
/// user's own balance — an inherent cost of the queue hint (bucket/round it below if that
/// must stay private).
pub struct NetworkWithdrawable {
	pub network: Network,
	pub withdrawable: Usdt,
	pub instant: Usdt,
	pub min_withdrawal: Usdt,
	pub withdrawal_fee: Usdt,
}

pub struct Wallet {
	pub balance: WalletBalance,
	pub deposit_addresses: Vec<DepositRail>,
	pub withdrawable: Vec<NetworkWithdrawable>,
}

/// The caller's wallet: the unified lifecycle balance, a deposit address per rail, and
/// the per-rail withdrawable view.
pub async fn get_wallet(
	ledger: &dyn Ledger,
	allocations: &dyn AllocationRepository,
	withdrawals: &dyn WithdrawalRepository,
	deposit_addresses: &dyn DepositAddresses,
	user: UserId,
) -> Result<Wallet, DomainError> {
	// Layer 1 — the single unified claim. The ledger speaks raw base units; wrap into
	// the typed `Usdt` at this boundary.
	let claim = ledger.balance(&LedgerAccountKey::UserClaim(user)).await?;
	let available = Usdt::from_base_units(claim.available());

	// invested = the user's active stakes (network-agnostic projection).
	let user_allocations = allocations.list_by_user(user).await?;
	let invested = user_allocations
		.iter()
		.filter(|a| a.state() == AllocationState::Active && matches!(a.kind(), AllocationKind::UserStake { .. }))
		.try_fold(Usdt::ZERO, |acc, a| acc.checked_add(a.amount()))
		.ok_or_else(|| DomainError::Repository("invested total overflow".into()))?;

	// pending_withdrawal = the gross of queued/in-flight withdrawals (projection).
	let user_withdrawals = withdrawals.list_by_user(user).await?;
	let pending_withdrawal = user_withdrawals
		.iter()
		.filter(|w| matches!(w.state(), WithdrawalState::Queued | WithdrawalState::Processing))
		.try_fold(Usdt::ZERO, |acc, w| acc.checked_add(w.amount()))
		.ok_or_else(|| DomainError::Repository("pending withdrawal total overflow".into()))?;

	let total = available
		.checked_add(invested)
		.and_then(|sum| sum.checked_add(pending_withdrawal))
		.ok_or_else(|| DomainError::Repository("wallet total overflow".into()))?;

	let balance = WalletBalance {
		available,
		invested,
		pending_withdrawal,
		total,
	};

	// Layer 2 — per-rail deposit addresses and withdrawable view.
	let mut deposit_addresses_out = Vec::with_capacity(Network::ALL.len());
	let mut withdrawable = Vec::with_capacity(Network::ALL.len());
	for network in Network::ALL {
		deposit_addresses_out.push(DepositRail {
			network,
			address: Some(deposit_addresses.address(user, network).await?),
		});
		// `instant` = min(available, rail liquidity) — "this much ships without queueing".
		// It reveals the rail's liquidity only up to the user's own balance (see the
		// NetworkWithdrawable doc); bucket/round here if that disclosure must be avoided.
		let rail_liquidity = Usdt::from_base_units(ledger.balance(&LedgerAccountKey::CryptoWallet(network)).await?.posted);
		withdrawable.push(NetworkWithdrawable {
			network,
			withdrawable: available,
			instant: available.min(rail_liquidity),
			min_withdrawal: WithdrawalPolicy::minimum(network),
			withdrawal_fee: WithdrawalPolicy::fee(network),
		});
	}

	Ok(Wallet {
		balance,
		deposit_addresses: deposit_addresses_out,
		withdrawable,
	})
}

/// The caller's deposit address on `network` (stable; derived once and reused).
pub async fn get_deposit_address(deposit_addresses: &dyn DepositAddresses, user: UserId, network: Network) -> Result<WalletAddress, DomainError> {
	deposit_addresses.address(user, network).await
}
