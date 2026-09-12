//! Persistence + read ports for the [`PaymentOrder`] aggregate.
//!
//! Every method is internally atomic and the method IS the transaction boundary — no caller
//! ever holds one across the port. Two of them carry the weight:
//!
//! - [`PaymentRepository::open`] writes the order, its materialized approval requirement
//!   (a consent seat, or the link to the consilium that will decide it) and the drained
//!   events in ONE transaction, so an order can never exist with no record of what would
//!   authorize it;
//! - [`PaymentRepository::submit`] counts the attempt, compares the code, records the
//!   decision and transitions the order **all under one `SELECT … FOR UPDATE` on the
//!   `payments` row** — not on the consent row, because the order is what the transition
//!   belongs to and locking the seat instead would let the approval and the order move in
//!   two transactions a retry could interleave.
//!
//! **TigerBeetle is never touched here.** A payment's money moves in the relay, from the
//! `Reserved`/`Settled` events these methods drain to the outbox — the one ACID point stays
//! Postgres, exactly as it does for every other money aggregate.

use async_trait::async_trait;
use domain::{
	balance::Party,
	consilium::ConsiliumId,
	error::DomainError,
	payments::{PaymentEffect, PaymentId, PaymentOrder, PaymentState},
	users::UserId,
};

/// The digest length every stored secret is reduced to, named so the schema's
/// `octet_length(...) = 32` checks and the Rust side cannot drift apart.
pub const DIGEST_BYTES: usize = 32;

/// How many wrong codes a consent seat may submit before its token burns permanently — and,
/// with it, the payment fails closed. The consilium's ceiling, deliberately: `docs/CONSILIUM.md`
/// § "One specification for both planes" is one specification, not shared code.
pub const MAX_CODE_ATTEMPTS: i32 = 5;

/// What must be written beside a brand-new order to materialize its one approval requirement.
///
/// A sum type rather than two optional fields: an order needs exactly one of these, decided by
/// [`domain::payments::PaymentTerms::requirement`], and a shape that could carry both or
/// neither would let an unauthorizable order commit.
pub enum ApprovalSeat {
	/// The source is fund-owned: the owner quorum that decides this order. The consilium
	/// itself is opened through [`ConsiliumRepository`](super::ConsiliumRepository); this is
	/// only the link, recorded so the order names its authorization from the moment it exists.
	Consilium(ConsiliumId),
	/// The source is one investor's claim: that investor's own emailed consent.
	Consent(ConsentCredential),
}

/// The minted consent seat — **digests only**. The plaintext token and code exist in the
/// application layer just long enough to be handed to the mailer, and are never stored, so a
/// dump of `payment_consent` yields nothing that can consent to anything.
///
/// NOT YET ATOMIC WITH ITS NOTIFICATION, and deliberately called out rather than implied.
/// `consilium_mail` enqueues the approval mail in the same transaction as the seat, so a
/// concierge outage can never leave a request nobody was told about. Payments has no
/// equivalent queue yet — the `payment_consent` mail kind is a concierge change that lands
/// before the RPCs do — so until then the seat commits and the notification is the caller's
/// to send. `payment_consent.notified` is the column that will carry it.
pub struct ConsentCredential {
	pub subject: UserId,
	pub token_hash: [u8; DIGEST_BYTES],
	pub code_hash: [u8; DIGEST_BYTES],
	/// The subject's folded revoke floor at open — `GREATEST(concierge_token_version,
	/// token_version)`, the same value [`IssuanceTarget::token_version`](super::IssuanceTarget)
	/// carries, so a revoke on either plane counts. Re-checked at consent and again at
	/// execution, which is what makes `RevokeTokens` void a consent that is already in flight.
	pub token_version_at_open: u64,
	/// SHA-256 over the subject's mirrored `users.email` at open, byte for byte as stored
	/// (already normalized by `Email::parse`). Re-checked at consent and at execution, so
	/// changing the mailbox at the identity provider cannot redirect a live token.
	pub email_hash_at_open: [u8; DIGEST_BYTES],
}

/// The consent seat as a surface renders it.
#[derive(Debug)]
pub struct ConsentView {
	pub subject: UserId,
	pub email: String,
	pub decision: ConsentDecision,
	/// Unix seconds; 0 while pending.
	pub decided_at: i64,
	pub notified: bool,
	pub attempts_remaining: u32,
	/// Why the seat can no longer be answered or executed — one of the pins recorded at open
	/// has moved — or `None` while it still holds. The execution path for an L1 order must
	/// read this BEFORE creating the withdrawal: `record_execution` refuses a moved pin too,
	/// but by then the withdrawal would already exist.
	pub invalidated: Option<String>,
}

/// What the emailed investor answered. The consilium's `VoteDecision` is the same three
/// values and is deliberately NOT reused: these are two planes that happen to agree today,
/// and folding them would make a future divergence a refactor of both.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConsentDecision {
	Pending,
	Approve,
	Reject,
}

impl ConsentDecision {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Pending => "pending",
			Self::Approve => "approve",
			Self::Reject => "reject",
		}
	}

	pub fn parse(raw: &str) -> Result<Self, DomainError> {
		match raw {
			"pending" => Ok(Self::Pending),
			"approve" => Ok(Self::Approve),
			"reject" => Ok(Self::Reject),
			other => Err(DomainError::Validation(format!("unknown consent decision: {other}"))),
		}
	}
}

/// An order and the identity slice a surface needs beside it.
#[derive(Debug)]
pub struct PaymentView {
	pub order: PaymentOrder,
	pub initiator_email: String,
	/// Present exactly when the requirement is the owner quorum.
	pub consilium_id: Option<ConsiliumId>,
	/// Present exactly when the requirement is the subject's consent.
	pub consent: Option<ConsentView>,
}

/// What the emailed investor is shown. Deliberately narrower than [`PaymentView`]: the terms
/// they are consenting to and nothing about the operator who proposed it beyond an address.
#[derive(Debug)]
pub struct ConsentInvitation {
	pub payment_id: PaymentId,
	pub state: PaymentState,
	pub order: PaymentOrder,
	pub payload_hash: String,
	pub initiator_email: String,
	pub subject_email: String,
	pub expires_at: i64,
	pub decision: ConsentDecision,
	pub attempts_remaining: u32,
}

/// Audit facts the edge supplies with a consent. Recorded, never trusted for authorization.
pub struct ConsentAudit {
	pub client_ip: String,
	pub user_agent: String,
}

/// The result of a consent that was actually accepted.
#[derive(Debug)]
pub struct ConsentOutcome {
	pub payment: PaymentView,
	/// True when this answer carried the order out of `pending`.
	pub decided: bool,
	/// True when that answer was approval — the signal to attempt execution.
	pub approved: bool,
}

/// How an execution attempt ended. Written by [`PaymentRepository::record_execution`].
pub enum ExecutionOutcome {
	Executed(PaymentEffect),
	Failed(String),
}

/// What an admin screen filters the payments table by. Every field is additive and `None`
/// means "no restriction", so the empty filter is the whole history.
#[derive(Default)]
pub struct PaymentFilter {
	pub state: Option<PaymentState>,
	/// Either end. Most payments have no user on either side (`Piggybank → Revenue`), which
	/// is exactly why the admin screen cannot be served by the per-user operation feed.
	pub party: Option<Party>,
	/// Only orders whose source is fund-owned (the governance surface), when true.
	pub fund_owned_source: Option<bool>,
}

/// The single response every unusable consent token produces, whatever made it unusable —
/// unknown, expired, spent, burned, or attached to an order that has closed. A caller cannot
/// tell which they hit, so the surface cannot be used to probe for live tokens.
pub fn consent_not_found() -> DomainError {
	DomainError::NotFound {
		entity: "consent",
		id: String::new(),
	}
}

#[async_trait]
pub trait PaymentRepository: Send + Sync {
	/// Persist a brand-new order together with its materialized approval requirement, in one
	/// transaction. Takes the source claim's write lock ([`lock_claim`](crate::infrastructure::outbox::lock_claim))
	/// first: a payment spends the same claims withdrawals and subscriptions do, and a
	/// spender that skips the lock silently stops the serialization covering the others.
	///
	/// Refuses with [`DomainError::Conflict`] when another order is already open against the
	/// same fund-owned source (the partial unique index is what actually enforces it).
	async fn open(&self, order: &mut PaymentOrder, seat: ApprovalSeat) -> Result<(), DomainError>;

	/// Load one order in full (no lock; for queries).
	async fn find(&self, id: PaymentId) -> Result<Option<PaymentView>, DomainError>;

	/// Record that the owner quorum carried this order — the consilium branch's counterpart
	/// to [`Self::submit`]. Applies [`PaymentOrder::approve`] under the order's row lock, so
	/// the reservation event and the state change commit together. Idempotent.
	async fn record_approval(&self, id: PaymentId, at: i64) -> Result<PaymentView, DomainError>;

	/// Record that the owner quorum refused, or that the order was withdrawn by its initiator.
	/// Under the row lock; idempotent on an already-closed order.
	async fn record_rejection(&self, id: PaymentId, at: i64) -> Result<PaymentView, DomainError>;

	/// Withdraw an order the caller opened, under the row lock. Refuses a caller who is not
	/// the initiator; idempotent on an already-cancelled one.
	async fn cancel(&self, id: PaymentId, by: UserId, at: i64) -> Result<PaymentView, DomainError>;

	/// Resolve an emailed consent token to its invitation. **Strictly side-effect free** —
	/// mail scanners issue automatic requests for every URL in a message, so this must not
	/// count an attempt, spend a token, or record anything.
	async fn invitation(&self, token_hash: &[u8; DIGEST_BYTES], at: i64) -> Result<ConsentInvitation, DomainError>;

	/// Answer a consent: count the attempt, compare the code in constant time, record the
	/// decision and transition the order — one transaction, one lock, taken on the `payments`
	/// row. A repeat of the same answer is an idempotent no-op; a different one is refused.
	///
	/// Burning the token (five wrong codes) **fails the payment closed**. With one seat there
	/// is no second party to escalate to, so the burn is a refusal, not a detector.
	///
	/// A seat whose pins have moved since open (the subject's sessions revoked, or their
	/// mailbox changed) is refused with [`DomainError::Conflict`] before the code is compared,
	/// and the order is rejected with it — fail-closed, and terminal for the same one-seat
	/// reason.
	async fn submit(&self, token_hash: &[u8; DIGEST_BYTES], code: &str, decision: ConsentDecision, audit: &ConsentAudit, at: i64) -> Result<ConsentOutcome, DomainError>;

	/// Expire every pending order past its deadline. Returns how many closed.
	async fn expire_due(&self, at: i64) -> Result<usize, DomainError>;

	/// Orders that are approved but have no effect yet — what an execution retry picks up
	/// after a crash between the approval and the money. An `execution_failed` one is
	/// deliberately NOT here: nothing retries silently.
	async fn awaiting_execution(&self) -> Result<Vec<PaymentId>, DomainError>;

	/// Record how the execution attempt ended, under the row lock. Writing the effect is
	/// idempotent for the same effect and a conflict for a different one — which is what lets
	/// the caller re-read by the deterministic id and believe the row rather than the error.
	///
	/// An `Executed` outcome over a consent seat whose pins have moved since open is NOT
	/// recorded: the order is moved to `execution_failed` (releasing an L2/L3 reservation)
	/// and the call returns [`DomainError::Conflict`] naming why. See
	/// [`ConsentView::invalidated`] for the L1 path, which must check before the withdrawal
	/// exists.
	async fn record_execution(&self, id: PaymentId, outcome: ExecutionOutcome, at: i64) -> Result<PaymentView, DomainError>;
}

/// The admin payments screen's read model.
///
/// A **query-side** port, separate from [`PaymentRepository`] for the reason
/// [`OperationFeed`](super::OperationFeed) is separate from the aggregates it reads: it owns
/// nothing and writes nothing. It exists as its own trait rather than another method on the
/// repository so the screen's permission and paging can be granted without handing out the
/// write surface with them.
#[async_trait]
pub trait PaymentFeed: Send + Sync {
	/// The payment history matching `filter`, newest first, capped at `limit`. Nothing is
	/// ever deleted, so rejected, expired and failed orders stay in it — the audit record is
	/// half the point of the screen.
	async fn list(&self, filter: &PaymentFilter, limit: i64) -> Result<Vec<PaymentView>, DomainError>;
}
