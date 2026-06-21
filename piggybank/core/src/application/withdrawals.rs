//! Withdrawal use cases — request + cancel (user), dispatch/settle/fail (operator),
//! list (user).
//!
//! `request_withdrawal` is a command with a **two-part Read-First**: it gates on the
//! user being active (the KYC/freeze seam), confirms the **available** unified claim
//! (posted − already-reserved) covers the gross (user solvency; the TB non-negative
//! flag is the backstop), then checks the **chosen rail's liquidity** (treasury) — if
//! the rail can cover the net it dispatches immediately, otherwise the withdrawal is
//! accepted and left `Queued` for the treasury worker (`dispatch_withdrawal`) to send
//! once the rail is topped up. `settle`/`fail` are the operator/watcher-driven
//! completions (admin-gated at the boundary), standing in for a chain watcher + custody
//! confirmation callback; `cancel` (user) refunds a still-queued withdrawal. The
//! cardinal rule — fail (void) only when the broadcast certainly did not land — is why
//! `fail` is only legal once `Processing`, while a `Queued` one is always safe to cancel.

use domain::{
	balance::LedgerAccountKey,
	error::DomainError,
	money::{Network, TxRef, Usdt, WalletAddress},
	users::UserId,
	withdrawals::{Withdrawal, WithdrawalId, WithdrawalPolicy},
};
use tokio::sync::Notify;

use crate::ports::{UserRepository, WithdrawalRepository, ledger::Ledger};

/// The calling user withdraws `amount` (gross) of free balance to `address`. The fee
/// is the per-network policy fee; the net (`amount − fee`) is what leaves on-chain.
#[allow(clippy::too_many_arguments)]
pub async fn request_withdrawal(
	withdrawals: &dyn WithdrawalRepository,
	ledger: &dyn Ledger,
	users: &dyn UserRepository,
	relay: &Notify,
	user: UserId,
	network: Network,
	address: WalletAddress,
	amount: Usdt,
) -> Result<Withdrawal, DomainError> {
	// KYC/freeze gate — a disabled account may not move money out.
	let account = users.find_by_id(user).await?.ok_or_else(|| DomainError::NotFound {
		entity: "user",
		id: user.to_string(),
	})?;
	if !account.is_active() {
		return Err(DomainError::Forbidden("account is not permitted to withdraw".into()));
	}
	let fee = WithdrawalPolicy::fee(network);
	// Validate the request shape (minimum, fee coverage, no on-chain dust, address net).
	let mut withdrawal = Withdrawal::request(WithdrawalId::new(), user, network, address, amount, fee)?;
	// Read-First #1 — user solvency: the spendable unified claim (posted minus what's
	// already reserved by other in-flight withdrawals) must cover the gross. TB's flag
	// is the hard backstop.
	let claim = ledger.balance(&LedgerAccountKey::UserClaim(user)).await?;
	if claim.available() < amount {
		return Err(DomainError::Validation("insufficient available balance to withdraw".into()));
	}
	// Read-First #2 — rail liquidity (treasury): if the chosen rail can cover the net
	// now, dispatch to custody immediately; otherwise accept and leave it queued for the
	// treasury worker to dispatch once the rail is topped up (accept-and-queue).
	let rail_liquidity = ledger.balance(&LedgerAccountKey::CryptoWallet(network)).await?.posted;
	if rail_liquidity >= withdrawal.net_amount() {
		withdrawal.dispatch()?;
	}
	withdrawals.open(&mut withdrawal).await?;
	relay.notify_one();
	Ok(withdrawal)
}

/// Dispatch a queued withdrawal to custody (treasury worker / admin): the chosen rail
/// now has liquidity, so the relay broadcasts. Idempotent.
pub async fn dispatch_withdrawal(withdrawals: &dyn WithdrawalRepository, relay: &Notify, id: WithdrawalId) -> Result<Withdrawal, DomainError> {
	let withdrawal = withdrawals.dispatch(id).await?;
	relay.notify_one();
	Ok(withdrawal)
}

/// Cancel a still-queued withdrawal (the calling user): voids the reservation,
/// refunding in full. Ownership is checked here; the aggregate refuses to cancel once
/// the withdrawal is processing (a broadcast may have landed).
pub async fn cancel_withdrawal(withdrawals: &dyn WithdrawalRepository, relay: &Notify, id: WithdrawalId, user: UserId) -> Result<Withdrawal, DomainError> {
	let existing = withdrawals.find_by_id(id).await?.ok_or_else(|| DomainError::NotFound {
		entity: "withdrawal",
		id: id.to_string(),
	})?;
	if existing.user() != user {
		return Err(DomainError::Forbidden("not your withdrawal".into()));
	}
	let withdrawal = withdrawals.cancel(id).await?;
	relay.notify_one();
	Ok(withdrawal)
}

/// Settle a confirmed withdrawal (operator/watcher): records the chain `tx_ref` and
/// posts the reservation, moving the net out of custody. Idempotent.
pub async fn settle_withdrawal(withdrawals: &dyn WithdrawalRepository, relay: &Notify, id: WithdrawalId, tx_ref: TxRef) -> Result<Withdrawal, DomainError> {
	let withdrawal = withdrawals.settle(id, tx_ref).await?;
	relay.notify_one();
	Ok(withdrawal)
}

/// Fail a processing withdrawal (operator/watcher): voids the reservation, refunding
/// the user. Only safe when the broadcast certainly did not reach the chain.
pub async fn fail_withdrawal(withdrawals: &dyn WithdrawalRepository, relay: &Notify, id: WithdrawalId) -> Result<Withdrawal, DomainError> {
	let withdrawal = withdrawals.fail(id).await?;
	relay.notify_one();
	Ok(withdrawal)
}

/// The calling user's withdrawals (projection), newest first.
pub async fn list_withdrawals(withdrawals: &dyn WithdrawalRepository, user: UserId) -> Result<Vec<Withdrawal>, DomainError> {
	withdrawals.list_by_user(user).await
}
