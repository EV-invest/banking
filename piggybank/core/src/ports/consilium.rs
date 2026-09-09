//! Persistence + read port for the [`Consilium`] aggregate.
//!
//! Every method is internally atomic. The two that matter most are [`ConsiliumRepository::open`]
//! and [`ConsiliumRepository::submit`]: `open` writes the consilium, its voter seats and their
//! approval mails in one transaction (so a mail outage can never leave a consilium nobody was
//! told about, and a mail can never exist for a consilium that failed to commit), and `submit`
//! counts the attempt, compares the code, tallies and transitions **all under one
//! `SELECT … FOR UPDATE`** on the consilium row — the tally and the verdict are never two
//! transactions a concurrent voter could interleave.

use async_trait::async_trait;
use domain::{
	consilium::{Consilium, ConsiliumId, ConsiliumState, RevenuePayoutTerms, VoteDecision},
	error::DomainError,
	users::UserId,
	withdrawals::WithdrawalId,
};

/// The digest length every stored secret is reduced to. Named so the schema's
/// `octet_length(...) = 32` checks and the Rust side cannot drift apart.
pub const DIGEST_BYTES: usize = 32;

/// How many wrong codes a seat may submit before its token burns permanently.
pub const MAX_CODE_ATTEMPTS: i32 = 5;

/// One minted seat, handed to [`ConsiliumRepository::open`]. The plaintexts travel no further
/// than the mail row the adapter writes from them; only the digests are stored on the seat.
pub struct VoterCredential {
	pub user_id: UserId,
	/// The opaque single-use token that goes in the emailed link.
	pub token: String,
	/// The secret code the owner types on the approval page.
	pub code: String,
	pub token_hash: [u8; DIGEST_BYTES],
	pub code_hash: [u8; DIGEST_BYTES],
}

/// A seat as a surface renders it. `email` is masked by the caller wherever a non-owner
/// could read it.
#[derive(Debug)]
pub struct VoterView {
	pub user_id: UserId,
	pub email: String,
	pub decision: VoteDecision,
	/// Unix seconds; 0 while pending.
	pub decided_at: i64,
	pub notified: bool,
}

/// A consilium and the identity slice a surface needs beside it.
#[derive(Debug)]
pub struct ConsiliumView {
	pub consilium: Consilium,
	pub initiator_email: String,
	pub voters: Vec<VoterView>,
}

/// What the emailed owner is shown. Deliberately narrower than [`ConsiliumView`]: no other
/// owner's identity and no vote-by-vote breakdown.
#[derive(Debug)]
pub struct InvitationView {
	pub consilium_id: ConsiliumId,
	pub state: ConsiliumState,
	pub terms: RevenuePayoutTerms,
	pub payload_hash: String,
	pub initiator_email: String,
	pub voter_email: String,
	pub threshold: u32,
	pub approvals: u32,
	pub owner_count: u32,
	pub created_at: i64,
	pub expires_at: i64,
	pub decision: VoteDecision,
	pub attempts_remaining: u32,
}

/// Audit facts the edge supplies with a vote. Recorded, never trusted for authorization.
pub struct VoteAudit {
	pub client_ip: String,
	pub user_agent: String,
}

/// The result of a vote that was actually accepted.
#[derive(Debug)]
pub struct SubmitOutcome {
	pub invitation: InvitationView,
	/// True when this vote carried the consilium to its verdict.
	pub decided: bool,
	/// True when that verdict was approval — the signal to attempt execution.
	pub approved: bool,
}

/// How an execution attempt ended. Written by [`ConsiliumRepository::record_execution`].
pub enum ExecutionOutcome {
	Executed(WithdrawalId),
	Failed(String),
}

/// The single response every unusable token produces, whatever made it unusable — unknown,
/// expired, spent, burned, or attached to a consilium that has closed. A caller cannot tell
/// which they hit, so the surface cannot be used to probe for live tokens.
pub fn invitation_not_found() -> DomainError {
	DomainError::NotFound {
		entity: "invitation",
		id: String::new(),
	}
}

#[async_trait]
pub trait ConsiliumRepository: Send + Sync {
	/// The fund's owners, from the locally mirrored roster (`users.role`). Read here and
	/// never from concierge: a payout must not need a live call to the identity plane at
	/// the moment it is authorized.
	async fn owner_roster(&self) -> Result<Vec<UserId>, DomainError>;

	/// Persist a new consilium, its voter seats, and one approval mail per seat — in one
	/// transaction. Refuses with [`DomainError::Conflict`] when another consilium is
	/// already open (the schema's partial unique index is what actually enforces it).
	async fn open(&self, consilium: &mut Consilium, credentials: &[VoterCredential], approval_url_base: &str) -> Result<(), DomainError>;

	/// Load one consilium in full (no lock; for queries).
	async fn find(&self, id: ConsiliumId) -> Result<Option<ConsiliumView>, DomainError>;

	/// The governance history, newest first. Nothing is deleted, so closed consilia stay in it.
	async fn list(&self, limit: i64) -> Result<Vec<ConsiliumView>, DomainError>;

	/// Withdraw a consilium the caller opened, under the row lock. Refuses a caller who is
	/// not the initiator; idempotent on an already-cancelled one.
	async fn cancel(&self, id: ConsiliumId, by: UserId, at: i64) -> Result<ConsiliumView, DomainError>;

	/// Resolve an emailed token to its invitation. **Strictly side-effect free** — mail
	/// scanners issue automatic requests for every URL in a message, so this must not
	/// count an attempt, spend a token, or record anything.
	async fn invitation(&self, token_hash: &[u8; DIGEST_BYTES], at: i64) -> Result<InvitationView, DomainError>;

	/// Cast a vote: count the attempt, compare the code in constant time, tally, and
	/// transition — one transaction, one row lock. A repeat of the same decision is an
	/// idempotent no-op; a different one is refused.
	async fn submit(&self, token_hash: &[u8; DIGEST_BYTES], code: &str, decision: VoteDecision, audit: &VoteAudit, at: i64) -> Result<SubmitOutcome, DomainError>;

	/// Expire every open consilium past its deadline, enqueueing the outcome mail for each.
	/// Returns how many closed.
	async fn expire_due(&self, at: i64) -> Result<usize, DomainError>;

	/// Consilia that reached quorum but have no payout yet — what an execution retry picks
	/// up after a crash between the verdict and the payout. An `execution_failed` one is
	/// deliberately NOT here: nothing retries silently.
	async fn awaiting_execution(&self) -> Result<Vec<ConsiliumId>, DomainError>;

	/// Record how the execution attempt ended, under the row lock, and enqueue the outcome
	/// mail. Writing the payout id is idempotent for the same id and a conflict for a
	/// different one.
	async fn record_execution(&self, id: ConsiliumId, outcome: ExecutionOutcome, at: i64) -> Result<ConsiliumView, DomainError>;

	/// When an owner last joined or left the roster, as a unix timestamp; `None` if the
	/// roster has never moved. Drives the cooling-off period — see
	/// [`ROSTER_COOLING_OFF_SECS`](crate::application::consilium::ROSTER_COOLING_OFF_SECS).
	async fn last_roster_change_at(&self) -> Result<Option<i64>, DomainError>;

	/// Void every open consilium that was opened before `changed_at`, telling its audience
	/// why. Returns how many closed. Idempotent: a consilium opened after the change is
	/// left alone, so re-running the sweep voids nothing twice.
	async fn void_open_for_roster_change(&self, changed_at: i64, at: i64) -> Result<usize, DomainError>;
}
