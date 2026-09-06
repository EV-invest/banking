//! Consilium use cases — open, cancel and read (owners); the emailed invitation and vote
//! (no session); and the execution that turns an approved consilium into a revenue payout.
//!
//! The consilium is a **separate aggregate** from the withdrawal it authorizes. It reserves
//! nothing and refunds nothing; on approval it calls the ordinary
//! [`request_revenue_payout`](crate::application::withdrawals::request_revenue_payout) path,
//! so the queue, the chain watchers, the dispatcher, the reaper and reconciliation cover the
//! resulting payout with no new machinery — and the money aggregate never learns that
//! governance exists.

use domain::{
	consilium::{Consilium, ConsiliumId, ConsiliumState, RevenuePayoutTerms, VoteDecision},
	error::DomainError,
	money::Network,
	users::UserId,
	withdrawals::WithdrawalId,
};
use tokio::sync::Notify;
use uuid::Uuid;

use crate::{
	application::withdrawals::{self as withdrawal_app, WithdrawalPorts},
	infrastructure::consilium::digest,
	ports::{
		Custody, WithdrawalRepository,
		consilium::{ConsiliumRepository, ConsiliumView, DIGEST_BYTES, ExecutionOutcome, InvitationView, SubmitOutcome, VoteAudit, VoterCredential},
		ledger::Ledger,
	},
};

/// Crockford base32 minus `I`, `L`, `O` and `U` — the four glyphs a human misreads or
/// mistypes. 32 symbols divides 256 exactly, so sampling a byte modulo the alphabet is
/// unbiased with no rejection loop.
const CODE_ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// 10 symbols over a 32-letter alphabet ≈ 50 bits — far beyond what five attempts can reach,
/// while still being something an owner will actually type off a screen.
const CODE_LEN: usize = 10;

/// 32 random bytes (256 bits), hex-encoded. Comfortably past the 24-byte floor, and the
/// token is single-use with a 72h TTL besides.
const TOKEN_BYTES: usize = 32;

/// The salt that makes the payout id a pure function of the consilium.
const PAYOUT_SALT: &[u8] = b"consilium:revenue-payout";

/// The driven ports the consilium write-path borrows. The withdrawal-side handles are here
/// only for the execution step — opening and voting touch no money at all.
pub struct ConsiliumPorts<'a> {
	pub consilia: &'a dyn ConsiliumRepository,
	pub withdrawals: &'a dyn WithdrawalRepository,
	pub ledger: &'a dyn Ledger,
	pub custody: &'a dyn Custody,
	pub relay: &'a Notify,
	pub configured: &'a [Network],
	/// Base URL the emailed approval link is built on.
	pub approval_url_base: &'a str,
	/// Whether a governance mailer is actually wired behind the `consilium_mail` queue.
	///
	/// A runtime fact rather than a `cfg!`, because it is the composition root that decides
	/// whether a mailer exists — and because a test needs to exercise both answers on one
	/// build. See [`require_governance_mail`].
	pub governance_mail_wired: bool,
}

impl ConsiliumPorts<'_> {
	fn withdrawal_ports(&self) -> WithdrawalPorts<'_> {
		WithdrawalPorts {
			withdrawals: self.withdrawals,
			ledger: self.ledger,
			custody: self.custody,
			relay: self.relay,
		}
	}
}

/// The payout a consilium will create: `uuid_v5(consilium_id, "consilium:revenue-payout")`.
///
/// Deterministic on purpose. A retried execution recomputes the same id, so the second
/// attempt finds the payout already there instead of opening a second one — the difference
/// between an at-least-once execution path and a double payout.
pub fn payout_id(consilium: ConsiliumId) -> WithdrawalId {
	WithdrawalId::from_raw(Uuid::new_v5(&consilium.raw(), PAYOUT_SALT))
}

/// Open a consilium over a proposed revenue payout.
///
/// The roster comes from the LOCAL mirror (`users.role`, maintained by the one-way bridge),
/// never a live call to concierge: authorizing a payout must not depend on the identity
/// plane being reachable at that moment. The terms are validated against the very same gates
/// the payout itself will face, so an impossible request is refused now rather than after
/// three owners have spent 72 hours approving it.
/// How long an admission to or removal from the owner roster blocks a new payout proposal.
///
/// WHY THIS EXISTS, AND WHAT IT DOES NOT FIX. The quorum rules compose weaker than any of
/// them reads on its own: at `N = 3` a removal carries on ONE peer's vote, so two colluding
/// owners can expel the third, admit puppets by unanimity, and then reach a payout quorum
/// entirely legitimately. No arrangement of thresholds fixes that — a majority of the roster
/// owns the roster, and that is what "majority" means.
///
/// What a cooling-off period buys is VISIBILITY. It makes the seizure and the payout two
/// separate events two days apart instead of one uninterrupted motion, and the delay is
/// itself a signal any remaining honest owner, operator or auditor can act on. The cost is
/// real and accepted: a legitimate roster change also delays a legitimate payout by 48h.
pub const ROSTER_COOLING_OFF_SECS: i64 = 48 * 60 * 60;

/// Refuse to open a payout proposal in the shadow of a roster change.
///
/// Paired with [`ConsiliumRepository::void_open_for_roster_change`], which closes the other
/// half: a change landing while a proposal is already open voids that proposal, so the
/// window cannot be straddled by opening first and changing the roster after.
async fn require_settled_roster(ports: &ConsiliumPorts<'_>, now: i64) -> Result<(), DomainError> {
	let Some(changed_at) = ports.consilia.last_roster_change_at().await? else {
		return Ok(());
	};
	let lifts_at = changed_at.saturating_add(ROSTER_COOLING_OFF_SECS);
	if now >= lifts_at {
		return Ok(());
	}
	let hours = (lifts_at - now).div_euclid(3600);
	let minutes = (lifts_at - now).rem_euclid(3600).div_euclid(60);
	Err(DomainError::Conflict(format!(
		"the owner roster changed less than {}h ago; a payout consilium cannot be opened until the cooling-off period lifts in {hours}h {minutes}m",
		ROSTER_COOLING_OFF_SECS / 3600
	)))
}

/// Refuse to open a consilium that nobody could ever vote on.
///
/// Every seat's approval token reaches its owner by exactly one route: the `consilium_mail`
/// queue, drained by [`ConsiliumMailer`](crate::infrastructure::consilium_mailer) into
/// concierge. On a build where that seam is not compiled in, the queue fills and is never
/// drained — so a consilium opens, mails nothing, and expires 72h later having been
/// unvotable from the first instant.
///
/// WHY REFUSE HERE AND NOT AT BOOT. Refusing at boot would take the whole money plane down —
/// deposits, withdrawals, subscriptions, reconciliation — over a feature that is off by
/// default and orthogonal to all of them; a self-inflicted outage is a worse failure than
/// the one it reports. Refusing here fails exactly the one operation that cannot work, names
/// why, and leaves every other path running. It is safe to be this strict only because the
/// direct single-admin payout RPC is now closed (see `services/balance.rs`): there is no
/// bypass to fall back on, so an operator who needs to pay revenue out has to wire the
/// mailer rather than route around governance.
fn require_governance_mail(wired: bool) -> Result<(), DomainError> {
	if wired {
		return Ok(());
	}
	Err(DomainError::Conflict(
		"governance mail is not configured, so no owner could be sent an approval token; a consilium opened now could never be voted on. Build with the `concierge_governance_mail` feature and configure the concierge mail relay first.".into(),
	))
}

pub async fn open_revenue_payout(ports: &ConsiliumPorts<'_>, initiator: UserId, terms: RevenuePayoutTerms, now: i64) -> Result<ConsiliumView, DomainError> {
	require_governance_mail(ports.governance_mail_wired)?;
	require_settled_roster(ports, now).await?;
	withdrawal_app::check_revenue_payout(ports.ledger, ports.configured, terms.network, terms.address.clone(), terms.amount).await?;
	let owners = ports.consilia.owner_roster().await?;
	let payload_hash = digest(&terms.canonical_bytes());
	let mut consilium = Consilium::open(ConsiliumId::new(), terms, payload_hash, initiator, &owners, now)?;
	// One token and one code per ELIGIBLE seat. The initiator is not among them, which is
	// what makes "the initiator cannot vote" a fact about what exists rather than a check
	// somewhere that could be forgotten.
	let credentials = consilium.eligible().iter().map(|voter| mint_credential(*voter)).collect::<Result<Vec<_>, _>>()?;
	ports.consilia.open(&mut consilium, &credentials, ports.approval_url_base).await?;
	find(ports.consilia, consilium.id()).await
}

/// Mint one seat's credentials. The plaintexts are returned to the caller (they have to
/// reach the owner's mailbox); only their digests are ever stored.
fn mint_credential(user_id: UserId) -> Result<VoterCredential, DomainError> {
	let mut token_bytes = [0u8; TOKEN_BYTES];
	let mut code_bytes = [0u8; CODE_LEN];
	getrandom::fill(&mut token_bytes).map_err(|_| DomainError::Repository("OS randomness unavailable".into()))?;
	getrandom::fill(&mut code_bytes).map_err(|_| DomainError::Repository("OS randomness unavailable".into()))?;
	let token = hex::encode(token_bytes);
	let code: String = code_bytes.iter().map(|byte| CODE_ALPHABET[(*byte % 32) as usize] as char).collect();
	Ok(VoterCredential {
		user_id,
		token_hash: digest(token.as_bytes()),
		code_hash: digest(code.as_bytes()),
		token,
		code,
	})
}

pub async fn find(consilia: &dyn ConsiliumRepository, id: ConsiliumId) -> Result<ConsiliumView, DomainError> {
	consilia.find(id).await?.ok_or_else(|| DomainError::NotFound {
		entity: "consilium",
		id: id.to_string(),
	})
}

pub async fn list(consilia: &dyn ConsiliumRepository, limit: i64) -> Result<Vec<ConsiliumView>, DomainError> {
	consilia.list(limit).await
}

/// Withdraw a consilium the caller opened. Votes are not carried anywhere: reopening starts
/// a fresh request with a fresh hash, which is what keeps an approval bound to the terms it
/// was given.
pub async fn cancel(consilia: &dyn ConsiliumRepository, id: ConsiliumId, by: UserId, now: i64) -> Result<ConsiliumView, DomainError> {
	consilia.cancel(id, by, now).await
}

/// The redacted invitation behind an emailed token. Side-effect free.
pub async fn invitation(consilia: &dyn ConsiliumRepository, token: &str, now: i64) -> Result<InvitationView, DomainError> {
	consilia.invitation(&token_digest(token), now).await
}

/// Cast a vote. The repository does the whole of it — attempt, comparison, tally and
/// transition — in one transaction under the consilium's row lock.
pub async fn submit_decision(consilia: &dyn ConsiliumRepository, token: &str, code: &str, decision: VoteDecision, audit: &VoteAudit, now: i64) -> Result<SubmitOutcome, DomainError> {
	consilia.submit(&token_digest(token), code, decision, audit, now).await
}

fn token_digest(token: &str) -> [u8; DIGEST_BYTES] {
	digest(token.as_bytes())
}

/// Turn an approved consilium into a revenue payout.
///
/// Six things make this safe to call more than once, from more than one place (the vote that
/// carried it, and the sweeper picking up an approval whose execution never ran):
///
/// 1. only an `approved` consilium is executable — an expired, rejected, cancelled or
///    already-failed one is refused, so a late vote can never reach the money;
/// 2. the payload hash is re-verified against the stored terms, so the payout that goes out
///    is the one the owners were shown;
/// 3. the tally is re-checked against the LIVE owner roster, so an approval that a seat
///    change has since invalidated cannot be spent;
/// 4. an approval that has been sitting too long past its window is refused rather than
///    executed on a stale authorization;
/// 5. the withdrawal id is derived from the consilium, so a retried execution re-creates the
///    same row rather than paying twice — and a refusal from the payout path is re-read
///    against that id before being believed, so the loser of a two-caller race records the
///    payout that actually exists instead of a phantom failure;
/// 6. a genuine refusal is recorded as `execution_failed` with its reason — terminal,
///    visible to the owners, and retried by nothing.
pub async fn execute(ports: &ConsiliumPorts<'_>, id: ConsiliumId, now: i64) -> Result<ConsiliumView, DomainError> {
	let view = find(ports.consilia, id).await?;
	let consilium = &view.consilium;
	if consilium.state() == ConsiliumState::Executed {
		return Ok(view);
	}
	if consilium.state() != ConsiliumState::Approved {
		return Err(DomainError::Conflict(format!("consilium is {}, not executable", consilium.state().as_str())));
	}
	// An approval is a signature over `payload_hash`. Re-taking the digest over what is
	// about to be paid is what makes an edited row unable to spend that signature.
	if digest(&consilium.terms().canonical_bytes()) != consilium.payload_hash() {
		let reason = "the stored terms no longer match the payload hash the owners approved".to_owned();
		return ports.consilia.record_execution(id, ExecutionOutcome::Failed(reason), now).await;
	}
	// THE QUORUM IS RE-CHECKED AGAINST THE LIVE ROSTER, NOT THE STORED VERDICT.
	//
	// `state = approved` is a fact about the tally at the moment the carrying vote landed.
	// Execution can happen much later — after a crash, after a transient failure the sweeper
	// only warned about, or simply in the next sweep window — and an owner can lose their
	// seat in between. `find()` recomputes `approvals()` through the live `still_an_owner`
	// join while `threshold()` stays frozen at the roster size the consilium opened against,
	// so this comparison is exactly "does a quorum of CURRENT owners still stand behind
	// this?". Without it the central invariant would hold at tally time and fail at the one
	// moment that actually spends money.
	if consilium.approvals() < consilium.threshold() {
		let reason = format!(
			"only {} of the {} required approvals are still held by current owners — the roster changed after the vote carried",
			consilium.approvals(),
			consilium.threshold()
		);
		return ports.consilia.record_execution(id, ExecutionOutcome::Failed(reason), now).await;
	}
	// AN APPROVAL GOES STALE TOO. `expire_due` only ever touches `open` consilia, so an
	// approved one has no deadline of its own: a payout approved just before a long outage
	// would otherwise execute whenever the process came back, on an authorization the owners
	// gave against a world that no longer exists. The grace is what keeps an ordinary sweep
	// gap or a brief restart from voiding a legitimately fresh approval.
	// A roster change during the request's life voids it. The sweeper does the voiding and
	// tells the owners, but it runs on a 60s cadence — so this refuses immediately rather
	// than leaving a window in which an approval given before the change could still be
	// spent after it.
	if let Some(changed_at) = ports.consilia.last_roster_change_at().await?
		&& changed_at >= consilium.created_at()
	{
		let reason = "the owner roster changed while this request was open, so the approval was voided".to_owned();
		return ports.consilia.record_execution(id, ExecutionOutcome::Failed(reason), now).await;
	}
	if now > consilium.expires_at().saturating_add(EXECUTION_GRACE_SECS) {
		let reason = "the approval went stale: execution did not run within the grace period after the voting window closed".to_owned();
		return ports.consilia.record_execution(id, ExecutionOutcome::Failed(reason), now).await;
	}
	let withdrawal = payout_id(id);
	let terms = consilium.terms().clone();
	if ports.withdrawals.find_by_id(withdrawal).await?.is_some() {
		return ports.consilia.record_execution(id, ExecutionOutcome::Executed(withdrawal), now).await;
	}
	let outcome = match withdrawal_app::request_revenue_payout(&ports.withdrawal_ports(), ports.configured, withdrawal, terms.network, terms.address, terms.amount).await {
		Ok(payout) => ExecutionOutcome::Executed(payout.id()),
		// A REFUSAL HERE IS NOT PROOF THE PAYOUT DOES NOT EXIST.
		//
		// Two callers reach this by construction: the inline execute after the carrying vote,
		// and the sweeper. Both can see `find_by_id == None` above; one then inserts and the
		// other loses on the `withdrawals` primary key. Recording the loser's error as
		// `Failed` would be a lie that sticks — the consilium would read `execution_failed`,
		// every owner would be mailed a failure, and `awaiting_execution` would never return
		// it again, all while the payout row exists and will be broadcast. So: re-read before
		// believing the error.
		Err(err) => match ports.withdrawals.find_by_id(withdrawal).await? {
			Some(_) => ExecutionOutcome::Executed(withdrawal),
			None => ExecutionOutcome::Failed(failure_reason(&err)),
		},
	};
	ports.consilia.record_execution(id, outcome, now).await
}

/// How long after the voting window closes an approval may still be spent.
///
/// Sized for the gap an ordinary incident leaves: the sweep runs every minute, so a day
/// covers a restart, a deploy, or a stretch of degraded execution without voiding an
/// approval the owners would still stand behind — while refusing one that has been sitting
/// through a multi-day outage.
const EXECUTION_GRACE_SECS: i64 = 24 * 60 * 60;

/// What the owners are told when a payout could not be created.
///
/// A validation refusal is the fund's own vocabulary — "payout exceeds the fund's available
/// revenue", "amount is below the rail minimum" — and is exactly what an owner needs in
/// order to act. Anything else is an infrastructure detail: a raw Postgres or TigerBeetle
/// string in a governance email tells the owners nothing they can use and leaks schema
/// internals to every seat on the roster. Those become one fixed sentence, and the detail
/// goes to the log where an operator will look for it.
fn failure_reason(err: &DomainError) -> String {
	match err {
		DomainError::Validation(_) | DomainError::Conflict(_) | DomainError::Forbidden(_) | DomainError::NotFound { .. } => err.to_string(),
		DomainError::Repository(detail) => {
			tracing::error!(detail = %detail, "consilium: payout creation failed on an infrastructure error");
			"the payout could not be created because of an internal error; an operator has been alerted".to_owned()
		}
	}
}

/// Close every consilium whose window has run out. Driven by the periodic sweeper.
pub async fn sweep_expired(consilia: &dyn ConsiliumRepository, now: i64) -> Result<usize, DomainError> {
	consilia.expire_due(now).await
}
