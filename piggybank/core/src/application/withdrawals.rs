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

/// The driven ports the withdrawal write-path borrows: the aggregate's repository, the
/// ledger both Read-First checks read, the custody gateway the rail-liquidity check asks,
/// and the relay nudged once the control-plane commit lands. Exactly the set
/// [`open_withdrawal`] — the shared body of both request paths — needs, so each use-case's
/// own parameters stay its *request*: which source, which rail, where, how much. The
/// user-facing entry point's extra gates (the [`UserRepository`] KYC/freeze check, the
/// configured-rail list) are deliberately NOT here: the revenue path has no user to gate,
/// and a field it could never use would only invite one. A plain borrow-holder: it owns
/// nothing and does nothing.
pub struct WithdrawalPorts<'a> {
	/// The `withdrawals` aggregate's driven port (Postgres control plane).
	pub withdrawals: &'a dyn WithdrawalRepository,
	/// The money gateway (TigerBeetle): the source's claim and the rail's accounting balance.
	pub ledger: &'a dyn Ledger,
	/// The custody gateway — read-only here, for the on-chain treasury view.
	pub custody: &'a dyn Custody,
	/// Nudged after the commit so the outbox relay broadcasts promptly.
	pub relay: &'a Notify,
}

/// The calling user withdraws `amount` (gross) of free balance to `address`. The fee
/// is the per-network policy fee; the net (`amount − fee`) is what leaves on-chain.
pub async fn request_withdrawal(
	ports: &WithdrawalPorts<'_>,
	users: &dyn UserRepository,
	configured: &[Network],
	user: UserId,
	network: Network,
	address: WalletAddress,
	amount: Usdt,
) -> Result<Withdrawal, DomainError> {
	require_configured(configured, network)?;
	// KYC/freeze gate — a disabled account may not move money out.
	let account = users.find_by_id(user).await?.ok_or_else(|| DomainError::NotFound {
		entity: "user",
		id: user.to_string(),
	})?;
	if !account.is_active() {
		return Err(DomainError::Forbidden("account is not permitted to withdraw".into()));
	}
	let source = WithdrawalSource::User(user);
	open_withdrawal(ports, WithdrawalId::new(), source, network, address, amount).await
}

/// Rail gate — the withdrawable view no longer offers an unconfigured rail, but a direct
/// API caller could otherwise queue a withdrawal that only a manual operator settle (the
/// stub custody fallthrough) could ever ship. Pre-existing withdrawals on a since-
/// de-configured rail stay listable/cancellable.
fn require_configured(configured: &[Network], network: Network) -> Result<(), DomainError> {
	if configured.contains(&network) {
		Ok(())
	} else {
		Err(DomainError::Validation(format!("{network} withdrawals are not available")))
	}
}

/// Would this revenue payout be accepted *right now*, without recording anything?
///
/// The consilium calls this at OPEN so an impossible payout — an unconfigured rail, a
/// sub-minimum amount, an address for the wrong chain, more than the fund has earned — is
/// refused before three owners spend 72 hours approving it. It runs the same three gates
/// [`open_withdrawal`] does, through the same code, so the answer cannot drift from what
/// execution will actually do. It is a *pre*-check, not a guarantee: revenue can still fall
/// between here and execution, which is what `ExecutionFailed` exists for.
pub async fn check_revenue_payout(ledger: &dyn Ledger, configured: &[Network], network: Network, address: WalletAddress, amount: Usdt) -> Result<(), DomainError> {
	require_configured(configured, network)?;
	let source = WithdrawalSource::Revenue;
	// `Withdrawal::request` IS the shape validator (minimum, fee coverage, on-chain dust,
	// address network), so the check is the constructor rather than a copy of its rules.
	Withdrawal::request(WithdrawalId::new(), source, network, address, amount, WithdrawalPolicy::fee_for(source, network))?;
	require_solvent(ledger, source, amount).await
}

/// Read-First on the source's claim: the spendable balance (posted minus what other
/// in-flight withdrawals have already reserved) must cover the gross. For a user that is
/// their unified claim; for a payout it is the fund's earned revenue, so this is the check
/// that makes "only what the fund earned" true rather than aspirational. TigerBeetle's
/// non-negative flag is the hard backstop either way.
async fn require_solvent(ledger: &dyn Ledger, source: WithdrawalSource, amount: Usdt) -> Result<(), DomainError> {
	let claim = ledger.balance(&source.claim_key()).await?;
	if Usdt::from_base_units(claim.available()) < amount {
		return Err(DomainError::Validation(if source.is_revenue() {
			"payout exceeds the fund's available revenue".into()
		} else {
			"insufficient available balance to withdraw".to_owned()
		}));
	}
	Ok(())
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
/// `id` is supplied by the caller so a consilium can derive it deterministically
/// (`uuid_v5(consilium_id, "consilium:revenue-payout")`) and have a retried execution
/// re-create the same row instead of a second payout. The ad-hoc admin path passes a fresh
/// [`WithdrawalId::new`].
pub async fn request_revenue_payout(
	ports: &WithdrawalPorts<'_>,
	configured: &[Network],
	id: WithdrawalId,
	network: Network,
	address: WalletAddress,
	amount: Usdt,
) -> Result<Withdrawal, DomainError> {
	require_configured(configured, network)?;
	open_withdrawal(ports, id, WithdrawalSource::Revenue, network, address, amount).await
}

/// The shared body of both request paths: validate the shape, Read-First the **source's**
/// solvency and the rail's liquidity, then record (dispatching straight away when the
/// rail can already cover it).
async fn open_withdrawal(ports: &WithdrawalPorts<'_>, id: WithdrawalId, source: WithdrawalSource, network: Network, address: WalletAddress, amount: Usdt) -> Result<Withdrawal, DomainError> {
	let fee = WithdrawalPolicy::fee_for(source, network);
	// Validate the request shape (minimum, fee coverage, no on-chain dust, address net).
	let mut withdrawal = Withdrawal::request(id, source, network, address, amount, fee)?;
	// Read-First #1 — the source can actually cover the gross.
	require_solvent(ports.ledger, source, amount).await?;
	// Read-First #2 — rail liquidity: dispatchable liquidity is `min(TB rail, on-chain
	// treasury)`. The TB `wallet:<net>` balance alone over-counts — it includes confirmed
	// deposits still sitting on users' derived addresses, which the treasury hot wallet
	// cannot spend. If the effective liquidity covers the net, dispatch to custody
	// immediately; otherwise accept and leave it queued for the dispatcher to send once
	// the rail is topped up (accept-and-queue). A treasury read failure also degrades to
	// queued — acceptance and the clearing reserve NEVER depend on rail liquidity, so a
	// flaky node must not refuse a user.
	let rail_liquidity = Usdt::from_base_units(ports.ledger.balance(&LedgerAccountKey::CryptoWallet(network)).await?.posted);
	let dispatchable = match ports.custody.treasury_liquidity(network).await {
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
	ports.withdrawals.open(&mut withdrawal).await?;
	ports.relay.notify_one();
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
