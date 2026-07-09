//! Wallet query use cases — a user's unified balance, deposit rails, and per-rail
//! withdrawal options.
//!
//! The user has **one** network-agnostic claim; the wallet presents it segmented by
//! lifecycle. Read-First: `available`, `pending_withdrawal`, and the cash side of `total`
//! all come live from the **same** TigerBeetle claim balance (the authoritative data
//! plane) — `available = posted − reserved`, `pending_withdrawal = reserved`, so
//! `available + pending_withdrawal == posted` by construction and the figures cannot
//! drift. `invested` is the sum of active stakes (a Postgres projection valued at NAV).
//! Network re-enters only as a transaction attribute: a per-rail deposit address and a
//! per-rail withdrawable view
//! (`instant = min(available, rail liquidity)`, the accept-and-queue degradation hint).
//!
//! Only **configured** rails (those with a running on-chain watcher) are presented or
//! provisioned at all: an unconfigured rail is omitted entirely — never provisioned,
//! not merely "address pending" — because a deposit sent to an address no watcher
//! scans is stranded, not credited.

use domain::{
	balance::LedgerAccountKey,
	error::DomainError,
	money::{Nav, Network, Shares, Usdt, WalletAddress},
	users::UserId,
	withdrawals::WithdrawalPolicy,
};

use crate::ports::{DepositAddresses, Deposits, FundPositionReader, NavMarks, deposits::DepositRecord, ledger::Ledger};

/// A user's single, network-agnostic balance, segmented by lifecycle. Every figure is
/// non-negative; `total = available + invested + pending_withdrawal`. `available` and
/// `pending_withdrawal` are two views of the same claim (`posted − reserved` and
/// `reserved`), so their sum is the claim's `posted` by construction — never a moment
/// where they diverge and `total` double-counts an in-flight withdrawal.
pub struct WalletBalance {
	/// Free, spendable now (claim posted − reserved).
	pub available: Usdt,
	/// Held in fund units, valued at the current NAV (`Σ units × NAV`).
	pub invested: Usdt,
	/// Reserved by in-flight withdrawals (the claim's `reserved` = Σ gross the relay has
	/// locked). Read from the ledger, not the `withdrawals` projection, so it stays in
	/// lockstep with `available` off one balance read.
	pub pending_withdrawal: Usdt,
	/// `available + invested + pending_withdrawal` — the user's whole position.
	pub total: Usdt,
}

/// A deposit rail — where to send USDT (on a given chain) to top up the unified
/// balance. Only configured rails appear; an unconfigured one is omitted entirely.
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

/// The caller's wallet: the unified lifecycle balance, a deposit address per
/// configured rail, and the per-rail withdrawable view.
pub async fn get_wallet(
	ledger: &dyn Ledger,
	positions: &dyn FundPositionReader,
	nav: &dyn NavMarks,
	deposit_addresses: &dyn DepositAddresses,
	configured: &[Network],
	user: UserId,
) -> Result<Wallet, DomainError> {
	// Layer 1 — the single unified claim. The ledger speaks raw base units; wrap into
	// the typed `Usdt` at this boundary. `available` and `pending_withdrawal` are the two
	// sides of this one balance (`posted − reserved` and `reserved`), so they can never
	// disagree about an in-flight withdrawal — the reserve (`Dr user / Cr clearing`
	// pending) is the only thing that locks a claim, and it moves both fields together.
	let claim = ledger.balance(&LedgerAccountKey::UserClaim(user)).await?;
	let available = Usdt::from_base_units(claim.available());
	// Sourced from `reserved`, NOT the `withdrawals` table: the projection counted a row
	// the instant it was created, while the ledger reserve applies asynchronously — so the
	// two summed as if simultaneously consistent and `total` transiently (or permanently,
	// if a reserve parked) overstated the claim by the gross.
	let pending_withdrawal = Usdt::from_base_units(claim.locked);

	// invested = the value of the user's fund positions: live units × current NAV.
	let mut invested = Usdt::ZERO;
	for position in positions.list(user).await? {
		let held = Shares::from_base_units(ledger.balance(&LedgerAccountKey::UserShares(position.service.clone(), user)).await?.posted);
		if held.is_zero() {
			continue;
		}
		let price = nav.current(&position.service).await?.map(|v| v.nav).unwrap_or(Nav::SEED);
		let value = price.value(held)?;
		invested = invested.checked_add(value).ok_or_else(|| DomainError::Repository("invested total overflow".into()))?;
	}

	// total is the whole claim (its settled `posted`, which still carries the reserved
	// gross as a pending debit until the withdrawal settles) plus invested — one balance
	// read, so `total == available + pending_withdrawal + invested` by construction.
	let total = Usdt::from_base_units(claim.posted)
		.checked_add(invested)
		.ok_or_else(|| DomainError::Repository("wallet total overflow".into()))?;

	let balance = WalletBalance {
		available,
		invested,
		pending_withdrawal,
		total,
	};

	// Layer 2 — per-rail deposit addresses and withdrawable view, configured rails only.
	let mut deposit_addresses_out = Vec::with_capacity(configured.len());
	let mut withdrawable = Vec::with_capacity(configured.len());
	for network in configured.iter().copied() {
		// `None` ⇒ no fundable address yet (still a placeholder): the rail is presented as
		// unavailable, never with an address that cannot actually receive funds.
		deposit_addresses_out.push(DepositRail {
			network,
			address: deposit_addresses.address(user, network).await?,
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

/// The caller's deposit address on `network` (stable; derived once and reused). `None`
/// while the address is still a placeholder — the rail is not yet fundable — or when
/// the rail is not configured at all.
pub async fn get_deposit_address(deposit_addresses: &dyn DepositAddresses, configured: &[Network], user: UserId, network: Network) -> Result<Option<WalletAddress>, DomainError> {
	// The gate must sit ABOVE the port: the first `DepositAddresses::address` call
	// provisions a signer keypair, and a key minted for a rail no watcher scans strands
	// whatever is deposited to it.
	if !configured.contains(&network) {
		return Ok(None);
	}
	deposit_addresses.address(user, network).await
}

/// The caller's credited on-chain deposits (projection), newest first.
pub async fn list_deposits(deposits: &dyn Deposits, user: UserId) -> Result<Vec<DepositRecord>, DomainError> {
	deposits.list_by_user(user).await
}
