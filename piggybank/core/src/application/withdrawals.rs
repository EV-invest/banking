//! Withdrawal use cases — request (user), settle/fail (operator), list (user).
//!
//! `request_withdrawal` is a command: it gates on the user being active (the KYC/
//! freeze seam), confirms the **available** claim (posted − already-reserved) covers
//! the gross amount Read-First (the TB non-negative flag is the backstop), records the
//! aggregate, and notifies the relay to reserve the money. `settle`/`fail` are the
//! operator/watcher-driven completions (admin-gated at the boundary) standing in for a
//! chain watcher + custody confirmation callback. The cardinal rule — fail (void) only
//! when the broadcast certainly did not land — is enforced socially at this seam, not
//! by the aggregate.

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
	// Read-First: the spendable balance (posted minus what's already reserved by other
	// in-flight withdrawals) must cover the gross. TB's flag is the hard backstop.
	let balance = ledger.balance(&LedgerAccountKey::UserClaim(user, network)).await?;
	if balance.available() < amount {
		return Err(DomainError::Validation("insufficient available balance to withdraw".into()));
	}
	withdrawals.open(&mut withdrawal).await?;
	relay.notify_one();
	Ok(withdrawal)
}

/// Settle a confirmed withdrawal (operator/watcher): records the chain `tx_ref` and
/// posts the reservations. Idempotent.
pub async fn settle_withdrawal(withdrawals: &dyn WithdrawalRepository, relay: &Notify, id: WithdrawalId, tx_ref: TxRef) -> Result<Withdrawal, DomainError> {
	let withdrawal = withdrawals.settle(id, tx_ref).await?;
	relay.notify_one();
	Ok(withdrawal)
}

/// Fail an unsettled withdrawal (operator/watcher): voids the reservation, refunding
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
