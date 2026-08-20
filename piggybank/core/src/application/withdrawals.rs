//! Withdrawal use cases — request + cancel (user), dispatch/settle/fail (operator),
//! list (user).
//!
//! `request_withdrawal` is a command with a **two-part Read-First**: it gates on the
//! user being active (the KYC/freeze seam), confirms the **available** unified claim
//! (posted − already-reserved) covers the gross (user solvency; the TB non-negative
//! flag is the backstop), then checks the **chosen rail's liquidity** — the min of the
//! TB rail accounting balance and the custody adapter's real on-chain treasury view —
//! and dispatches immediately when it covers the net, otherwise the withdrawal is
//! accepted and left `Queued` for the [`Dispatcher`](crate::infrastructure::dispatcher)
//! worker (or the admin `dispatch_withdrawal`) to send once the rail is topped up.
//! `settle`/`fail` are the operator/watcher-driven
//! completions (admin-gated at the boundary), standing in for a chain watcher + custody
//! confirmation callback; `cancel` (user) refunds a still-queued withdrawal. The
//! cardinal rule — fail (void) only when the broadcast certainly did not land — is why
//! `fail` is only legal once `Processing`, while a `Queued` one is always safe to cancel.

use domain::{
	balance::LedgerAccountKey,
	error::DomainError,
	money::{Network, TxRef, Usdt, WalletAddress},
	users::UserId,
	withdrawals::{Withdrawal, WithdrawalId, WithdrawalPolicy, WithdrawalSource},
};
use tokio::sync::Notify;
use tracing::warn;

use crate::ports::{Custody, UserRepository, WithdrawalRepository, ledger::Ledger};

/// The calling user withdraws `amount` (gross) of free balance to `address`. The fee
/// is the per-network policy fee; the net (`amount − fee`) is what leaves on-chain.
#[allow(clippy::too_many_arguments)]
pub async fn request_withdrawal(
	withdrawals: &dyn WithdrawalRepository,
	ledger: &dyn Ledger,
	users: &dyn UserRepository,
	custody: &dyn Custody,
	relay: &Notify,
	configured: &[Network],
	user: UserId,
	network: Network,
	address: WalletAddress,
	amount: Usdt,
) -> Result<Withdrawal, DomainError> {
	// Rail gate — the withdrawable view no longer offers an unconfigured rail, but a
	// direct API caller could otherwise queue a withdrawal that only a manual operator
	// settle (the stub custody fallthrough) could ever ship. Pre-existing withdrawals
	// on a since-de-configured rail stay listable/cancellable.
	if !configured.contains(&network) {
		return Err(DomainError::Validation(format!("{network} withdrawals are not available")));
	}
	// KYC/freeze gate — a disabled account may not move money out.
	let account = users.find_by_id(user).await?.ok_or_else(|| DomainError::NotFound {
		entity: "user",
		id: user.to_string(),
	})?;
	if !account.is_active() {
		return Err(DomainError::Forbidden("account is not permitted to withdraw".into()));
	}
	let source = WithdrawalSource::User(user);
	open_withdrawal(withdrawals, ledger, custody, relay, source, network, address, amount).await
}

/// The fund pays **its own earned revenue** out to `address` — the admin/owner payout.
///
/// Identical to a user withdrawal but for the claim it debits: `fee`, which holds what
/// the fund earned (retained withdrawal fees, plus any fee accrual crediting the same
/// account). Client money (`user:*`/`service:*`) and the fund's seed capital (`fund`)
/// are different accounts and are unreachable from here — not by a filter that could be
/// forgotten, but because [`WithdrawalSource::Revenue`] names exactly one account and
/// TigerBeetle's non-negative flag on it is the backstop.
///
/// Deliberately NOT gated on `configured` rails alone doing the work: like a user
/// withdrawal, an underfunded rail queues rather than refusing (the dispatcher ships it
/// on the next top-up), so a payout is never lost to a transient treasury dip.
#[allow(clippy::too_many_arguments)]
pub async fn request_revenue_payout(
	withdrawals: &dyn WithdrawalRepository,
	ledger: &dyn Ledger,
	custody: &dyn Custody,
	relay: &Notify,
	configured: &[Network],
	network: Network,
	address: WalletAddress,
	amount: Usdt,
) -> Result<Withdrawal, DomainError> {
	if !configured.contains(&network) {
		return Err(DomainError::Validation(format!("{network} withdrawals are not available")));
	}
	open_withdrawal(withdrawals, ledger, custody, relay, WithdrawalSource::Revenue, network, address, amount).await
}

/// The shared body of both request paths: validate the shape, Read-First the **source's**
/// solvency and the rail's liquidity, then record (dispatching straight away when the
/// rail can already cover it).
#[allow(clippy::too_many_arguments)]
async fn open_withdrawal(
	withdrawals: &dyn WithdrawalRepository,
	ledger: &dyn Ledger,
	custody: &dyn Custody,
	relay: &Notify,
	source: WithdrawalSource,
	network: Network,
	address: WalletAddress,
	amount: Usdt,
) -> Result<Withdrawal, DomainError> {
	let fee = WithdrawalPolicy::fee_for(source, network);
	// Validate the request shape (minimum, fee coverage, no on-chain dust, address net).
	let mut withdrawal = Withdrawal::request(WithdrawalId::new(), source, network, address, amount, fee)?;
	// Read-First #1 — source solvency: the spendable claim (posted minus what's already
	// reserved by other in-flight withdrawals) must cover the gross. For a user that is
	// their unified claim; for a payout it is the fund's earned revenue, so this is the
	// check that makes "only what the fund earned" true rather than aspirational. TB's
	// flag is the hard backstop either way.
	let claim = ledger.balance(&source.claim_key()).await?;
	if Usdt::from_base_units(claim.available()) < amount {
		return Err(DomainError::Validation(if source.is_revenue() {
			"payout exceeds the fund's available revenue".into()
		} else {
			"insufficient available balance to withdraw".to_owned()
		}));
	}
	// Read-First #2 — rail liquidity: dispatchable liquidity is `min(TB rail, on-chain
	// treasury)`. The TB `wallet:<net>` balance alone over-counts — it includes confirmed
	// deposits still sitting on users' derived addresses, which the treasury hot wallet
	// cannot spend. If the effective liquidity covers the net, dispatch to custody
	// immediately; otherwise accept and leave it queued for the dispatcher to send once
	// the rail is topped up (accept-and-queue). A treasury read failure also degrades to
	// queued — acceptance and the clearing reserve NEVER depend on rail liquidity, so a
	// flaky node must not refuse a user.
	let rail_liquidity = Usdt::from_base_units(ledger.balance(&LedgerAccountKey::CryptoWallet(network)).await?.posted);
	let dispatchable = match custody.treasury_liquidity(network).await {
		Ok(Some(onchain)) => rail_liquidity.min(onchain) >= withdrawal.net_amount(),
		// No chain view (stub / unwired rail) — the TB accounting balance is all there is.
		Ok(None) => rail_liquidity >= withdrawal.net_amount(),
		Err(err) => {
			warn!(%network, "treasury liquidity read failed — accepting the withdrawal queued: {err}");
			false
		}
	};
	if dispatchable {
		withdrawal.dispatch()?;
	}
	withdrawals.open(&mut withdrawal).await?;
	relay.notify_one();
	Ok(withdrawal)
}

/// Dispatch a queued withdrawal to custody (the dispatcher worker / admin): the chosen
/// rail now has liquidity, so the relay broadcasts. Refused — left queued, still
/// user-cancellable — when the rail treasury provably lacks the net on-chain (a dispatch
/// would only park at the custody backstop). `None`/`Err` reads dispatch as before: the
/// operator RPC is backed by human judgment, and stub rails stay operator-settled.
/// Idempotent.
pub async fn dispatch_withdrawal(withdrawals: &dyn WithdrawalRepository, custody: &dyn Custody, relay: &Notify, id: WithdrawalId) -> Result<Withdrawal, DomainError> {
	let existing = withdrawals.find_by_id(id).await?.ok_or_else(|| DomainError::NotFound {
		entity: "withdrawal",
		id: id.to_string(),
	})?;
	if let Ok(Some(onchain)) = custody.treasury_liquidity(existing.network()).await
		&& onchain < existing.net_amount()
	{
		return Err(DomainError::Validation("rail treasury underfunded on-chain — withdrawal left queued".into()));
	}
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
	if existing.user() != Some(user) {
		return Err(DomainError::Forbidden("not your withdrawal".into()));
	}
	let withdrawal = withdrawals.cancel(id).await?;
	relay.notify_one();
	Ok(withdrawal)
}

/// Cancel a still-queued **revenue payout** (admin): voids the reservation, returning
/// the gross to the fund's revenue claim. The mirror of [`cancel_withdrawal`] for the
/// source that has no user to own it — and it checks the source for the same reason
/// that one checks ownership: this entry point must not become a way for an admin to
/// cancel an investor's withdrawal out from under them.
pub async fn cancel_revenue_payout(withdrawals: &dyn WithdrawalRepository, relay: &Notify, id: WithdrawalId) -> Result<Withdrawal, DomainError> {
	let existing = withdrawals.find_by_id(id).await?.ok_or_else(|| DomainError::NotFound {
		entity: "withdrawal",
		id: id.to_string(),
	})?;
	if !existing.source().is_revenue() {
		return Err(DomainError::Forbidden("not a revenue payout".into()));
	}
	let withdrawal = withdrawals.cancel(id).await?;
	relay.notify_one();
	Ok(withdrawal)
}

/// The fund's own revenue payouts, newest first — the admin payout history.
pub async fn list_revenue_payouts(withdrawals: &dyn WithdrawalRepository) -> Result<Vec<Withdrawal>, DomainError> {
	withdrawals.list_revenue_payouts().await
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
