//! Withdrawal use cases — request + cancel (user), dispatch/settle/fail (operator),
//! list (user).
//!
//! `request_withdrawal` is a command with a **two-part Read-First**: it gates on the
//! user being active and KYC-verified (the freeze/verification seam), confirms the **available** unified claim
//! (posted − already-reserved) covers the gross (user solvency; the TB non-negative
//! flag is the backstop), then checks the **chosen rail's liquidity** — the min of the
//! TB rail accounting balance and the custody adapter's real on-chain treasury view —
//! and dispatches immediately when it covers the net, otherwise the withdrawal is
//! accepted and left `Queued` for the [`Dispatcher`](crate::infrastructure::dispatcher)
//! worker (or the admin `dispatch_withdrawal`) to send once the rail is topped up.
//! Admission is therefore NOT the last word on policy: `dispatch_withdrawal` re-evaluates
//! the kill-switch, the freeze and the tier-1 floor at the moment the money leaves, so a
//! pause, an AML hold or a revoked verification landing on an already-queued withdrawal
//! still stops it — whichever path (sweep or admin RPC) reaches it first. The tier floor
//! specifically is the deployment's [`KycGate`] (enforced by default), and the SAME value
//! must reach both points: a gate lifted only at admission would accept withdrawals that
//! dispatch then parks indefinitely.
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

use crate::{
	config::KycGate,
	ports::{Custody, OutflowPolicy, UserRepository, WithdrawalRepository, ledger::Ledger},
};

/// The driven ports the withdrawal write-path borrows: the aggregate's repository, the
/// ledger both Read-First checks read, the custody gateway the rail-liquidity check asks,
/// and the relay nudged once the control-plane commit lands. Exactly the set
/// [`open_withdrawal`] — the shared body of both request paths — needs, so each use-case's
/// own parameters stay its *request*: which source, which rail, where, how much. The
/// user-facing entry point's extra gates (the [`UserRepository`] KYC/freeze check, the
/// configured-rail list, the verification switch) are deliberately NOT here but in
/// [`AdmissionGates`]: the revenue path has no user to gate, and a field it could never
/// use would only invite one. A plain borrow-holder: it owns nothing and does nothing.
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

/// The gates the user-facing entry point runs before the shared body — exactly the set
/// [`WithdrawalPorts`] deliberately leaves out. They travel together because they are
/// answered together, at admission, about one caller: is this rail run at all, is the
/// account active, and does the verification floor apply to it ([`KycGate`], enforced
/// unless the deployment lifted it).
///
/// Bundled rather than passed loose for the same reason
/// [`DepositAddressPorts`](crate::application::wallet::DepositAddressPorts) is a bundle:
/// it keeps the use case's own parameters its *request* — whose withdrawal, which rail,
/// where, how much.
pub struct AdmissionGates<'a> {
	/// The control-plane row the freeze flag and the mirrored KYC tier are read from.
	pub users: &'a dyn UserRepository,
	/// The rails with a running on-chain watcher; a withdrawal on any other is refused.
	pub configured: &'a [Network],
	/// The deployment's verification gate. The SAME value must reach
	/// [`dispatch_withdrawal`]: a gate lifted only here admits withdrawals that the payout
	/// gate then leaves queued indefinitely.
	pub kyc: KycGate,
}

/// The calling user withdraws `amount` (gross) of free balance to `address`. The fee
/// is the per-network policy fee; the net (`amount − fee`) is what leaves on-chain.
///
/// `id` is supplied by the caller for the reason [`request_revenue_payout`]'s is: a payment
/// order derives it from itself (`uuid_v5(payment_id, "payment:withdrawal")`) so a retried
/// execution re-creates the same row instead of a second withdrawal. The self-service wallet
/// passes a fresh [`WithdrawalId::new`].
pub async fn request_withdrawal(
	ports: &WithdrawalPorts<'_>,
	gates: &AdmissionGates<'_>,
	id: WithdrawalId,
	user: UserId,
	network: Network,
	address: WalletAddress,
	amount: Usdt,
) -> Result<Withdrawal, DomainError> {
	admit_user_withdrawal(gates, user, network).await?;
	let source = WithdrawalSource::User(user);
	open_withdrawal(ports, id, source, network, address, amount).await
}

/// The user-facing admission gates, on their own: the rail is run, the account is active,
/// and the verification floor admits it. Shared by [`request_withdrawal`] and the payment
/// order's open-time pre-check, so an L1 payment out of an investor's claim is refused at
/// open for exactly the reasons its execution would refuse it 72 hours later.
async fn admit_user_withdrawal(gates: &AdmissionGates<'_>, user: UserId, network: Network) -> Result<(), DomainError> {
	require_configured(gates.configured, network)?;
	// KYC/freeze gate — a disabled account may not move money out.
	let account = gates.users.find_by_id(user).await?.ok_or_else(|| DomainError::NotFound {
		entity: "user",
		id: user.to_string(),
	})?;
	if !account.is_active() {
		return Err(DomainError::Forbidden("account is not permitted to withdraw".into()));
	}
	// Verification gate — an unverified account (tier 0 is a registration and a confirmed
	// email, nothing more) may not move money off the platform. The tier is the identity
	// plane's, mirrored onto the local row by the lifecycle bridge; this is the money
	// plane enforcing it. Deliberately absent from `request_revenue_payout`: that pays the
	// fund's own earned revenue out and has no user behind it to verify. The same `gate`
	// must reach `dispatch_withdrawal` too — a lifted gate that admits a withdrawal the
	// dispatch gate then parks forever is worse than no switch at all.
	if !gates.kyc.admits(account.kyc_level()) {
		return Err(DomainError::Forbidden("identity verification required to withdraw".into()));
	}
	Ok(())
}

/// Would this user withdrawal be accepted *right now*, without recording anything? The
/// user-side twin of [`check_revenue_payout`], run by a payment order at OPEN so an
/// impossible L1 payment is refused before its subject spends 72 hours consenting to it.
pub async fn check_user_withdrawal(ledger: &dyn Ledger, gates: &AdmissionGates<'_>, user: UserId, network: Network, address: WalletAddress, amount: Usdt) -> Result<(), DomainError> {
	admit_user_withdrawal(gates, user, network).await?;
	let source = WithdrawalSource::User(user);
	Withdrawal::request(WithdrawalId::new(), source, network, address, amount, WithdrawalPolicy::fee_for(source, network))?;
	require_solvent(ledger, source, amount).await
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

/// The global outflow pause — the read-only kill-switch — as a gate.
///
/// Public so the dispatcher can short-circuit a whole sweep on it. That is not a second
/// copy of the rule: [`dispatch_withdrawal`] calls this same function per withdrawal and
/// is the only thing standing between a queued row and the chain. The sweep's early exit
/// exists so a backlog of N under an operator pause costs one read and one log line every
/// interval instead of N of each.
pub async fn require_outflows_enabled(policy: &dyn OutflowPolicy) -> Result<(), DomainError> {
	if policy.outflows_paused().await? {
		return Err(DomainError::Forbidden("money movements are temporarily paused (read-only mode)".into()));
	}
	Ok(())
}

/// The outflow policy gate, re-evaluated at the moment of dispatch rather than trusted
/// from admission.
///
/// The three policies that guard a payout — the kill-switch, the cross-plane freeze and
/// the tier-1 floor — were all one-shot admission checks, and a queued withdrawal can sit
/// in the backlog for hours: an operator pause, an AML freeze or a revoked verification
/// that lands after acceptance must still stop the money. Every arm fails **closed**,
/// because by this point the gross is already reserved and the next step is a broadcast:
/// a missing owner row, an unreadable flag and a corrupt tier all refuse.
///
/// A revenue payout is exempt from the per-owner arms, and not by omission: the fund is
/// not a user, so there is no row to read and nothing to fail closed on. The kill-switch
/// still applies — it pauses outflows, not users.
async fn require_dispatchable(policy: &dyn OutflowPolicy, gate: KycGate, withdrawal: &Withdrawal) -> Result<(), DomainError> {
	require_outflows_enabled(policy).await?;
	let Some(owner) = withdrawal.user() else { return Ok(()) };
	let standing = policy
		.standing(owner)
		.await?
		.ok_or_else(|| DomainError::Forbidden("the withdrawal's owner has no control-plane row — dispatch refused".into()))?;
	if standing.blocked {
		return Err(DomainError::Forbidden("account is frozen".into()));
	}
	if !gate.admits(standing.kyc_level) {
		return Err(DomainError::Forbidden("identity verification required to withdraw".into()));
	}
	Ok(())
}

/// Dispatch a queued withdrawal to custody (the dispatcher worker / admin): the chosen
/// rail now has liquidity, so the relay broadcasts. Refused — left queued, still
/// user-cancellable — when the outflow policy no longer permits the payout (see
/// [`require_dispatchable`]) or when the rail treasury provably lacks the net on-chain (a
/// dispatch would only park at the custody backstop). `None`/`Err` from the chain view
/// reads dispatch as before: the operator RPC is backed by human judgment, and stub rails
/// stay operator-settled. Idempotent.
///
/// This is where the policy lives *because* this is the single funnel every payout passes
/// through — the sweep and the admin RPC both call it, so neither can inherit a weaker
/// rule than the other.
pub async fn dispatch_withdrawal(
	withdrawals: &dyn WithdrawalRepository,
	custody: &dyn Custody,
	policy: &dyn OutflowPolicy,
	gate: KycGate,
	relay: &Notify,
	id: WithdrawalId,
) -> Result<Withdrawal, DomainError> {
	let existing = withdrawals.find_by_id(id).await?.ok_or_else(|| DomainError::NotFound {
		entity: "withdrawal",
		id: id.to_string(),
	})?;
	require_dispatchable(policy, gate, &existing).await?;
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
