//! `consilium` bounded context — multi-owner authorization for the fund paying its OWN
//! earned revenue out.
//!
//! A consilium is an **authorization artifact**, not a money move. It reserves nothing,
//! queues nothing and refunds nothing; on approval it opens an ordinary revenue payout
//! through the existing withdrawal path. That separation is deliberate — see
//! `docs/CONSILIUM.md` — and it is why this aggregate adds no state to
//! [`crate::withdrawals::WithdrawalState`] and touches no claim.
//!
//! **Quorum.** `threshold = floor(N / 2) + 1` where `N` is the owner count at open. The
//! initiator is counted in the denominator but casts no vote: were opening a request to
//! remove you from the denominator, opening one would be a way to lower the bar you have
//! to clear. Below three owners the threshold is unreachable and [`Consilium::open`]
//! refuses rather than storing a request that can never pass.
//!
//! **Rejection closes early.** Once more owners have refused than the tally can afford,
//! the threshold is arithmetically out of reach, so the consilium is rejected at once
//! rather than left to run out its clock.
//!
//! Pure and wasm-safe: ids and the payload hash are supplied by the application layer,
//! no clock, no I/O, no crypto.

use ev::architecture::{AggregateRoot, DomainEvent, EmitsEvents, Entity, Id};
use serde::{Deserialize, Serialize};

use crate::{
	error::DomainError,
	money::{Network, Usdt, WalletAddress},
	users::UserId,
	withdrawals::WithdrawalId,
};

/// A unique consilium id (UUID). Minted by the application layer.
pub type ConsiliumId = Id<ConsiliumTag>;
/// Phantom tag making [`ConsiliumId`] a distinct, incompatible identity type.
pub struct ConsiliumTag;

/// The smallest roster that can ever reach the threshold. With `N = 2` the threshold is
/// 2 and there is exactly one eligible voter, so no vote can carry it.
pub const MIN_OWNERS: u32 = 3;

/// How long an unanswered consilium stays open (72h). Past it the request can never
/// execute, however late a vote arrives.
pub const TTL_SECS: i64 = 72 * 60 * 60;

/// The longest memo an initiator may attach. Long enough for a real justification, short
/// enough that the approval mail still reads as one.
pub const MAX_MEMO_BYTES: usize = 500;

/// Owners needed to carry a consilium: strictly more than half of ALL of them.
pub fn threshold(owner_count: u32) -> u32 {
	owner_count / 2 + 1
}

/// What a consilium authorizes. One kind today, named rather than implied so a second
/// governance subject would be a variant instead of a parallel aggregate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsiliumKind {
	RevenuePayout,
}

impl ConsiliumKind {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::RevenuePayout => "revenue_payout",
		}
	}

	pub fn parse(raw: &str) -> Result<Self, DomainError> {
		match raw {
			"revenue_payout" => Ok(Self::RevenuePayout),
			other => Err(DomainError::Validation(format!("unknown consilium kind: {other}"))),
		}
	}
}

/// Where a consilium stands. Terminal everywhere except [`Open`](ConsiliumState::Open).
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsiliumState {
	/// Collecting votes.
	Open,
	/// Quorum reached; the payout has not been created yet.
	Approved,
	/// Enough owners refused that the threshold can no longer be reached.
	Rejected,
	/// The window closed with no verdict.
	Expired,
	/// The initiator withdrew it.
	Cancelled,
	/// The payout exists.
	Executed,
	/// Approved, but the payout could not be created. Nothing retries silently.
	ExecutionFailed,
}

impl ConsiliumState {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Open => "open",
			Self::Approved => "approved",
			Self::Rejected => "rejected",
			Self::Expired => "expired",
			Self::Cancelled => "cancelled",
			Self::Executed => "executed",
			Self::ExecutionFailed => "execution_failed",
		}
	}

	pub fn parse(raw: &str) -> Result<Self, DomainError> {
		match raw {
			"open" => Ok(Self::Open),
			"approved" => Ok(Self::Approved),
			"rejected" => Ok(Self::Rejected),
			"expired" => Ok(Self::Expired),
			"cancelled" => Ok(Self::Cancelled),
			"executed" => Ok(Self::Executed),
			"execution_failed" => Ok(Self::ExecutionFailed),
			other => Err(DomainError::Validation(format!("unknown consilium state: {other}"))),
		}
	}

	pub fn is_open(self) -> bool {
		matches!(self, Self::Open)
	}
}

/// How one owner answered.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VoteDecision {
	#[default]
	Pending,
	Approve,
	Reject,
}

impl VoteDecision {
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
			other => Err(DomainError::Validation(format!("unknown vote decision: {other}"))),
		}
	}
}

/// The immutable subject of a revenue-payout consilium. There is no edit path: changing
/// anything means cancel and reopen, and votes are not carried over.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RevenuePayoutTerms {
	pub network: Network,
	pub address: WalletAddress,
	pub amount: Usdt,
	pub memo: String,
}

impl RevenuePayoutTerms {
	/// The domain-separation prefix. Included in the digest so a hash over these terms
	/// can never collide with one taken over some other message.
	const DOMAIN: &'static [u8] = b"banking.v1.RevenuePayoutTerms\x00";

	pub fn new(network: Network, address: WalletAddress, amount: Usdt, memo: String) -> Result<Self, DomainError> {
		if address.network() != network {
			return Err(DomainError::Validation("payout address is for a different network".into()));
		}
		if memo.len() > MAX_MEMO_BYTES {
			return Err(DomainError::Validation(format!("memo exceeds {MAX_MEMO_BYTES} bytes")));
		}
		// A control character has no meaning in a memo and every meaning in a mail header.
		if memo.chars().any(char::is_control) {
			return Err(DomainError::Validation("memo may not contain control characters".into()));
		}
		Ok(Self { network, address, amount, memo })
	}

	/// The bytes the payload hash is taken over: a fixed field order with every
	/// variable-length part length-prefixed, so no two distinct terms can encode alike
	/// and no serializer's map ordering can enter into it.
	pub fn canonical_bytes(&self) -> Vec<u8> {
		let mut out = Vec::with_capacity(Self::DOMAIN.len() + 96);
		out.extend_from_slice(Self::DOMAIN);
		push_field(&mut out, self.network.as_str().as_bytes());
		push_field(&mut out, self.address.as_str().as_bytes());
		out.extend_from_slice(&self.amount.base_units().to_be_bytes());
		push_field(&mut out, self.memo.as_bytes());
		out
	}
}

fn push_field(out: &mut Vec<u8>, bytes: &[u8]) {
	out.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
	out.extend_from_slice(bytes);
}

/// One recorded answer.
///
/// `counts` is the roster re-check: a vote cast by someone who has since lost their seat
/// stays on the record and stays visible, but is not counted toward quorum. Changing the
/// roster can therefore only make approval harder.
#[derive(Clone, Copy, Debug)]
pub struct ConsiliumVote {
	pub voter: UserId,
	pub decision: VoteDecision,
	pub decided_at: i64,
	pub counts: bool,
}

/// The consilium aggregate. Construct via [`Consilium::open`] (raises
/// [`ConsiliumEvent::Opened`]) or [`Consilium::rehydrate`] (load from the store, no
/// events).
#[derive(Clone, Debug)]
pub struct Consilium {
	id: ConsiliumId,
	kind: ConsiliumKind,
	terms: RevenuePayoutTerms,
	payload_hash: [u8; 32],
	initiator: UserId,
	owner_count: u32,
	threshold: u32,
	eligible: Vec<UserId>,
	votes: Vec<ConsiliumVote>,
	state: ConsiliumState,
	created_at: i64,
	expires_at: i64,
	decided_at: Option<i64>,
	executed_withdrawal_id: Option<WithdrawalId>,
	failure_reason: Option<String>,
	version: u64,
	pending: Vec<ConsiliumEvent>,
}

impl Consilium {
	/// Open a consilium over `terms`, snapshotting `owners` as the roster it is judged
	/// against. `payload_hash` is the SHA-256 of [`RevenuePayoutTerms::canonical_bytes`],
	/// taken by the application layer (the domain stays free of crypto).
	///
	/// Refuses a roster below [`MIN_OWNERS`]: with the initiator barred from voting the
	/// threshold would be unreachable, and a request that can never pass should not be
	/// stored as though it might.
	pub fn open(id: ConsiliumId, terms: RevenuePayoutTerms, payload_hash: [u8; 32], initiator: UserId, owners: &[UserId], created_at: i64) -> Result<Self, DomainError> {
		let mut roster: Vec<UserId> = Vec::with_capacity(owners.len());
		for owner in owners {
			if !roster.contains(owner) {
				roster.push(*owner);
			}
		}
		if !roster.contains(&initiator) {
			return Err(DomainError::Forbidden("only a fund owner may open a consilium".into()));
		}
		let owner_count = roster.len() as u32;
		if owner_count < MIN_OWNERS {
			return Err(DomainError::Validation(format!(
				"a payout consilium needs at least {MIN_OWNERS} owners; this fund has {owner_count}, so the threshold can never be reached"
			)));
		}
		let eligible: Vec<UserId> = roster.into_iter().filter(|owner| *owner != initiator).collect();
		let mut consilium = Self {
			id,
			kind: ConsiliumKind::RevenuePayout,
			terms,
			payload_hash,
			initiator,
			owner_count,
			threshold: threshold(owner_count),
			eligible,
			votes: Vec::new(),
			state: ConsiliumState::Open,
			created_at,
			expires_at: created_at.saturating_add(TTL_SECS),
			decided_at: None,
			executed_withdrawal_id: None,
			failure_reason: None,
			version: 1,
			pending: Vec::new(),
		};
		consilium.pending.push(ConsiliumEvent::Opened {
			consilium_id: id,
			kind: consilium.kind,
			terms: consilium.terms.clone(),
			payload_hash: hex32(&payload_hash),
			initiator,
			owner_count,
			threshold: consilium.threshold,
			expires_at: consilium.expires_at,
		});
		Ok(consilium)
	}

	/// Reconstitute from the store. Raises no events.
	#[allow(clippy::too_many_arguments)]
	pub fn rehydrate(
		id: ConsiliumId,
		kind: ConsiliumKind,
		terms: RevenuePayoutTerms,
		payload_hash: [u8; 32],
		initiator: UserId,
		owner_count: u32,
		threshold: u32,
		eligible: Vec<UserId>,
		votes: Vec<ConsiliumVote>,
		state: ConsiliumState,
		created_at: i64,
		expires_at: i64,
		decided_at: Option<i64>,
		executed_withdrawal_id: Option<WithdrawalId>,
		failure_reason: Option<String>,
		version: u64,
	) -> Self {
		Self {
			id,
			kind,
			terms,
			payload_hash,
			initiator,
			owner_count,
			threshold,
			eligible,
			votes,
			state,
			created_at,
			expires_at,
			decided_at,
			executed_withdrawal_id,
			failure_reason,
			version,
			pending: Vec::new(),
		}
	}

	/// Record `voter`'s answer, then settle the tally — which may carry the consilium
	/// straight to its verdict.
	///
	/// One-shot: repeating the SAME decision is an idempotent no-op (a retried request
	/// must not error), while a DIFFERENT second decision is refused. A vote from someone
	/// with no seat is forbidden, which is the second line of defence behind never having
	/// minted them a token at all.
	pub fn record_vote(&mut self, voter: UserId, decision: VoteDecision, at: i64) -> Result<(), DomainError> {
		if decision == VoteDecision::Pending {
			return Err(DomainError::Validation("a vote must be approve or reject".into()));
		}
		if !self.eligible.contains(&voter) {
			return Err(DomainError::Forbidden("not an eligible voter on this consilium".into()));
		}
		if let Some(existing) = self.votes.iter().find(|vote| vote.voter == voter) {
			return if existing.decision == decision {
				Ok(())
			} else {
				Err(DomainError::Conflict("this seat has already answered; a decision cannot be changed".into()))
			};
		}
		if !self.state.is_open() {
			return Err(DomainError::Conflict(format!("consilium is {}, no longer accepting votes", self.state.as_str())));
		}
		self.votes.push(ConsiliumVote {
			voter,
			decision,
			decided_at: at,
			counts: true,
		});
		self.raise(ConsiliumEvent::VoteRecorded {
			consilium_id: self.id,
			voter,
			decision,
			decided_at: at,
		});
		self.settle_tally(at);
		Ok(())
	}

	/// Carry the consilium to its verdict if the counted votes now decide it. Approval
	/// wins the moment the threshold is met; rejection wins the moment the threshold has
	/// become arithmetically unreachable, so the owners are not left waiting out a clock
	/// on a request that has already failed.
	fn settle_tally(&mut self, at: i64) {
		if !self.state.is_open() {
			return;
		}
		if self.approvals() >= self.threshold {
			self.decide(ConsiliumState::Approved, at);
		} else if self.rejections() > (self.eligible.len() as u32).saturating_sub(self.threshold) {
			self.decide(ConsiliumState::Rejected, at);
		}
	}

	fn decide(&mut self, state: ConsiliumState, at: i64) {
		self.state = state;
		self.decided_at = Some(at);
		let event = match state {
			ConsiliumState::Approved => ConsiliumEvent::Approved {
				consilium_id: self.id,
				approvals: self.approvals(),
				threshold: self.threshold,
				decided_at: at,
			},
			ConsiliumState::Rejected => ConsiliumEvent::Rejected {
				consilium_id: self.id,
				rejections: self.rejections(),
				decided_at: at,
			},
			ConsiliumState::Cancelled => ConsiliumEvent::Cancelled {
				consilium_id: self.id,
				decided_at: at,
			},
			_ => ConsiliumEvent::Expired {
				consilium_id: self.id,
				decided_at: at,
			},
		};
		self.raise(event);
	}

	/// Quorum has been reached. Normally driven by [`Self::record_vote`]; exposed so the
	/// transition is nameable. Idempotent.
	pub fn approve(&mut self, at: i64) -> Result<(), DomainError> {
		self.close_open(ConsiliumState::Approved, at)
	}

	/// The threshold is out of reach. Idempotent.
	pub fn reject(&mut self, at: i64) -> Result<(), DomainError> {
		self.close_open(ConsiliumState::Rejected, at)
	}

	/// The initiator withdrew the request. Ownership of the request is checked by the
	/// caller; the aggregate only guarantees it is still open. Idempotent.
	pub fn cancel(&mut self, at: i64) -> Result<(), DomainError> {
		self.close_open(ConsiliumState::Cancelled, at)
	}

	/// The window closed with no verdict. Refuses to expire a consilium early, so a
	/// clock skew cannot cut a vote short. Idempotent.
	pub fn expire(&mut self, at: i64) -> Result<(), DomainError> {
		if self.state.is_open() && at < self.expires_at {
			return Err(DomainError::Conflict("consilium has not expired yet".into()));
		}
		self.close_open(ConsiliumState::Expired, at)
	}

	/// The shared body of the four verdicts reachable from `open`: idempotent on repeat,
	/// a conflict from any other state.
	fn close_open(&mut self, target: ConsiliumState, at: i64) -> Result<(), DomainError> {
		if self.state == target {
			return Ok(());
		}
		if !self.state.is_open() {
			return Err(DomainError::Conflict(format!("consilium is {}, not {}", self.state.as_str(), target.as_str())));
		}
		self.decide(target, at);
		Ok(())
	}

	/// The approved payout now exists. Written exactly once — a repeat naming the SAME
	/// withdrawal is the idempotent retry an at-least-once execution path depends on, and
	/// one naming a different withdrawal is a conflict rather than a silent overwrite.
	pub fn mark_executed(&mut self, withdrawal_id: WithdrawalId, at: i64) -> Result<(), DomainError> {
		if self.state == ConsiliumState::Executed {
			return if self.executed_withdrawal_id == Some(withdrawal_id) {
				Ok(())
			} else {
				Err(DomainError::Conflict("consilium already executed a different payout".into()))
			};
		}
		if self.state != ConsiliumState::Approved {
			return Err(DomainError::Conflict(format!("consilium is {}, not executable", self.state.as_str())));
		}
		self.state = ConsiliumState::Executed;
		self.executed_withdrawal_id = Some(withdrawal_id);
		self.raise(ConsiliumEvent::Executed {
			consilium_id: self.id,
			withdrawal_id,
			at,
		});
		Ok(())
	}

	/// The approved payout could not be created. Terminal: nothing retries silently, and
	/// the reason is what an owner reads to understand why.
	pub fn mark_execution_failed(&mut self, reason: String, at: i64) -> Result<(), DomainError> {
		if self.state == ConsiliumState::ExecutionFailed {
			return Ok(());
		}
		if self.state != ConsiliumState::Approved {
			return Err(DomainError::Conflict(format!("consilium is {}, not executable", self.state.as_str())));
		}
		self.state = ConsiliumState::ExecutionFailed;
		self.failure_reason = Some(reason.clone());
		self.raise(ConsiliumEvent::ExecutionFailed { consilium_id: self.id, reason, at });
		Ok(())
	}

	fn raise(&mut self, event: ConsiliumEvent) {
		self.version = self.version.saturating_add(1);
		self.pending.push(event);
	}

	/// Approvals that still count — cast by someone who is still an owner.
	pub fn approvals(&self) -> u32 {
		self.counted(VoteDecision::Approve)
	}

	/// Rejections that still count.
	pub fn rejections(&self) -> u32 {
		self.counted(VoteDecision::Reject)
	}

	fn counted(&self, decision: VoteDecision) -> u32 {
		self.votes.iter().filter(|vote| vote.counts && vote.decision == decision).count() as u32
	}

	pub fn id(&self) -> ConsiliumId {
		self.id
	}

	pub fn kind(&self) -> ConsiliumKind {
		self.kind
	}

	pub fn terms(&self) -> &RevenuePayoutTerms {
		&self.terms
	}

	pub fn payload_hash(&self) -> [u8; 32] {
		self.payload_hash
	}

	/// The hash as the mail and the wire show it.
	pub fn payload_hash_hex(&self) -> String {
		hex32(&self.payload_hash)
	}

	pub fn initiator(&self) -> UserId {
		self.initiator
	}

	pub fn owner_count(&self) -> u32 {
		self.owner_count
	}

	pub fn threshold(&self) -> u32 {
		self.threshold
	}

	/// The seats entitled to vote — the roster at open, minus the initiator.
	pub fn eligible(&self) -> &[UserId] {
		&self.eligible
	}

	pub fn votes(&self) -> &[ConsiliumVote] {
		&self.votes
	}

	pub fn state(&self) -> ConsiliumState {
		self.state
	}

	pub fn created_at(&self) -> i64 {
		self.created_at
	}

	pub fn expires_at(&self) -> i64 {
		self.expires_at
	}

	pub fn decided_at(&self) -> Option<i64> {
		self.decided_at
	}

	pub fn executed_withdrawal_id(&self) -> Option<WithdrawalId> {
		self.executed_withdrawal_id
	}

	pub fn failure_reason(&self) -> Option<&str> {
		self.failure_reason.as_deref()
	}

	pub fn version(&self) -> u64 {
		self.version
	}
}

/// Lowercase hex of a digest. Hand-rolled so `domain` keeps its dependency set (and its
/// wasm-safety) unchanged for four lines of work.
fn hex32(bytes: &[u8; 32]) -> String {
	const DIGITS: &[u8; 16] = b"0123456789abcdef";
	let mut out = String::with_capacity(64);
	for byte in bytes {
		out.push(DIGITS[(byte >> 4) as usize] as char);
		out.push(DIGITS[(byte & 0x0f) as usize] as char);
	}
	out
}

/// Facts raised by the [`Consilium`] aggregate. Audit-only — a consilium moves no money,
/// so these land in `event_log` and never in the `outbox` (the relay has no ledger op for
/// the kind and would park the row).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConsiliumEvent {
	Opened {
		consilium_id: ConsiliumId,
		kind: ConsiliumKind,
		terms: RevenuePayoutTerms,
		payload_hash: String,
		initiator: UserId,
		owner_count: u32,
		threshold: u32,
		expires_at: i64,
	},
	VoteRecorded {
		consilium_id: ConsiliumId,
		voter: UserId,
		decision: VoteDecision,
		decided_at: i64,
	},
	Approved {
		consilium_id: ConsiliumId,
		approvals: u32,
		threshold: u32,
		decided_at: i64,
	},
	Rejected {
		consilium_id: ConsiliumId,
		rejections: u32,
		decided_at: i64,
	},
	Expired {
		consilium_id: ConsiliumId,
		decided_at: i64,
	},
	Cancelled {
		consilium_id: ConsiliumId,
		decided_at: i64,
	},
	Executed {
		consilium_id: ConsiliumId,
		withdrawal_id: WithdrawalId,
		at: i64,
	},
	ExecutionFailed {
		consilium_id: ConsiliumId,
		reason: String,
		at: i64,
	},
}

impl DomainEvent for ConsiliumEvent {
	const KIND: &'static str = "consilium";
}

impl Entity for Consilium {
	type Id = ConsiliumId;

	fn id(&self) -> ConsiliumId {
		self.id
	}
}

impl AggregateRoot for Consilium {
	const NAME: &'static str = "consilium";
}

impl EmitsEvents for Consilium {
	type Event = ConsiliumEvent;

	fn drain_events(&mut self) -> Vec<ConsiliumEvent> {
		core::mem::take(&mut self.pending)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	const NOW: i64 = 1_700_000_000;

	fn owners(n: usize) -> Vec<UserId> {
		(0..n).map(|_| UserId::new()).collect()
	}

	fn terms() -> RevenuePayoutTerms {
		let address = WalletAddress::parse(Network::Bep20, "0x52908400098527886E0F7030069857D2E4169EE7").unwrap();
		RevenuePayoutTerms::new(Network::Bep20, address, Usdt::parse_decimal("500").unwrap(), "quarterly draw".to_owned()).unwrap()
	}

	fn opened(roster: &[UserId]) -> Consilium {
		let mut c = Consilium::open(ConsiliumId::new(), terms(), [7u8; 32], roster[0], roster, NOW).unwrap();
		c.drain_events();
		c
	}

	#[test]
	fn threshold_is_strictly_more_than_half_for_every_roster() {
		// The quorum table in docs/CONSILIUM.md, asserted rather than described. `voters`
		// is N-1 because the initiator is in the denominator but casts no vote.
		let expected: [(u32, u32, u32); 7] = [
			// (N, threshold, voters)
			(2, 2, 1),
			(3, 2, 2),
			(4, 3, 3),
			(5, 3, 4),
			(6, 4, 5),
			(7, 4, 6),
			(8, 5, 7),
		];
		for (n, want_threshold, want_voters) in expected {
			assert_eq!(threshold(n), want_threshold, "threshold for N={n}");
			assert_eq!(n - 1, want_voters, "voters for N={n}");
			assert!(2 * want_threshold > n, "N={n}: the threshold must be strictly more than half");
		}
		// N=2 is the row that cannot work: one voter, two needed.
		assert!(threshold(2) > 2 - 1, "with two owners the single voter can never reach the threshold");
	}

	#[test]
	fn a_roster_below_three_is_refused_at_open() {
		for n in [1usize, 2] {
			let roster = owners(n);
			let err = Consilium::open(ConsiliumId::new(), terms(), [0u8; 32], roster[0], &roster, NOW).unwrap_err();
			assert!(matches!(err, DomainError::Validation(_)), "N={n} must be refused, not stored");
		}
		// N=3 is the smallest roster that CAN open — pinned here so the refusal above is
		// known to be about the roster size and not about some unrelated gate. (This line
		// previously called `owners(3)` twice, so the initiator was not in the roster it was
		// checked against; it passed on `Forbidden` while claiming to prove N=3 was refused,
		// which is the opposite of the policy.)
		let roster = owners(MIN_OWNERS as usize);
		assert!(
			Consilium::open(ConsiliumId::new(), terms(), [0u8; 32], roster[0], &roster, NOW).is_ok(),
			"N={MIN_OWNERS} is the smallest roster that can reach a threshold"
		);
	}

	#[test]
	fn only_an_owner_may_open() {
		let roster = owners(4);
		let outsider = UserId::new();
		let err = Consilium::open(ConsiliumId::new(), terms(), [0u8; 32], outsider, &roster, NOW).unwrap_err();
		assert!(matches!(err, DomainError::Forbidden(_)));
	}

	#[test]
	fn the_initiator_is_never_an_eligible_voter() {
		let roster = owners(5);
		let c = opened(&roster);
		assert_eq!(c.owner_count(), 5);
		assert_eq!(c.threshold(), 3);
		assert_eq!(c.eligible().len(), 4);
		assert!(!c.eligible().contains(&roster[0]), "the initiator holds no seat");
	}

	#[test]
	fn five_owners_need_three_of_four_voters() {
		let roster = owners(5);
		let mut c = opened(&roster);
		c.record_vote(roster[1], VoteDecision::Approve, NOW).unwrap();
		c.record_vote(roster[2], VoteDecision::Approve, NOW).unwrap();
		assert_eq!(c.state(), ConsiliumState::Open, "two of four is short of the threshold");
		c.record_vote(roster[3], VoteDecision::Approve, NOW).unwrap();
		assert_eq!(c.state(), ConsiliumState::Approved);
		assert_eq!(c.approvals(), 3);
	}

	#[test]
	fn four_owners_need_all_three_voters() {
		let roster = owners(4);
		let mut c = opened(&roster);
		assert_eq!(c.threshold(), 3);
		c.record_vote(roster[1], VoteDecision::Approve, NOW).unwrap();
		c.record_vote(roster[2], VoteDecision::Approve, NOW).unwrap();
		assert_eq!(c.state(), ConsiliumState::Open);
		c.record_vote(roster[3], VoteDecision::Approve, NOW).unwrap();
		assert_eq!(c.state(), ConsiliumState::Approved, "unanimity among the three peers");
	}

	#[test]
	fn three_owners_need_both_peers() {
		let roster = owners(3);
		let mut c = opened(&roster);
		assert_eq!(c.threshold(), 2);
		c.record_vote(roster[1], VoteDecision::Approve, NOW).unwrap();
		assert_eq!(c.state(), ConsiliumState::Open);
		c.record_vote(roster[2], VoteDecision::Approve, NOW).unwrap();
		assert_eq!(c.state(), ConsiliumState::Approved);
	}

	#[test]
	fn rejection_closes_the_moment_the_threshold_is_unreachable() {
		// N=5: 4 voters, threshold 3, so the tally can afford exactly one refusal.
		let roster = owners(5);
		let mut c = opened(&roster);
		c.record_vote(roster[1], VoteDecision::Reject, NOW).unwrap();
		assert_eq!(c.state(), ConsiliumState::Open, "three of the remaining four could still carry it");
		c.record_vote(roster[2], VoteDecision::Reject, NOW).unwrap();
		assert_eq!(c.state(), ConsiliumState::Rejected, "only two voters remain and three are needed");

		// N=3 and N=4 need unanimity, so a single refusal ends it immediately.
		for n in [3usize, 4] {
			let roster = owners(n);
			let mut c = opened(&roster);
			c.record_vote(roster[1], VoteDecision::Reject, NOW).unwrap();
			assert_eq!(c.state(), ConsiliumState::Rejected, "N={n} needs every peer");
		}
	}

	#[test]
	fn a_vote_is_one_shot_and_idempotent_on_repeat() {
		let roster = owners(5);
		let mut c = opened(&roster);
		c.record_vote(roster[1], VoteDecision::Approve, NOW).unwrap();
		c.drain_events();
		// The same answer again changes nothing and raises nothing.
		c.record_vote(roster[1], VoteDecision::Approve, NOW + 1).unwrap();
		assert!(c.drain_events().is_empty());
		assert_eq!(c.approvals(), 1);
		// A different answer is refused outright.
		let err = c.record_vote(roster[1], VoteDecision::Reject, NOW + 2).unwrap_err();
		assert!(matches!(err, DomainError::Conflict(_)));
		assert_eq!(c.rejections(), 0);
	}

	#[test]
	fn a_non_voter_is_refused_and_pending_is_not_a_vote() {
		let roster = owners(4);
		let mut c = opened(&roster);
		// The initiator holds no seat, so their own vote has nowhere to land.
		assert!(matches!(c.record_vote(roster[0], VoteDecision::Approve, NOW), Err(DomainError::Forbidden(_))));
		assert!(matches!(c.record_vote(UserId::new(), VoteDecision::Approve, NOW), Err(DomainError::Forbidden(_))));
		assert!(matches!(c.record_vote(roster[1], VoteDecision::Pending, NOW), Err(DomainError::Validation(_))));
	}

	#[test]
	fn a_voter_who_lost_their_seat_stops_counting() {
		let roster = owners(5);
		let mut c = opened(&roster);
		c.record_vote(roster[1], VoteDecision::Approve, NOW).unwrap();
		c.record_vote(roster[2], VoteDecision::Approve, NOW).unwrap();
		assert_eq!(c.approvals(), 2);
		// Rehydrating with that seat voided — what the repository does when the live
		// roster no longer lists them — takes the tally back below the threshold.
		let votes = c
			.votes()
			.iter()
			.map(|vote| ConsiliumVote {
				counts: vote.voter != roster[1],
				..*vote
			})
			.collect();
		let reloaded = Consilium::rehydrate(
			c.id(),
			c.kind(),
			c.terms().clone(),
			c.payload_hash(),
			c.initiator(),
			c.owner_count(),
			c.threshold(),
			c.eligible().to_vec(),
			votes,
			c.state(),
			c.created_at(),
			c.expires_at(),
			c.decided_at(),
			c.executed_withdrawal_id(),
			None,
			c.version(),
		);
		assert_eq!(reloaded.approvals(), 1, "a vote from someone with no seat does not count");
	}

	#[test]
	fn cancel_expire_and_the_verdicts_are_idempotent_and_mutually_exclusive() {
		let roster = owners(4);

		let mut cancelled = opened(&roster);
		cancelled.cancel(NOW).unwrap();
		assert_eq!(cancelled.state(), ConsiliumState::Cancelled);
		cancelled.drain_events();
		cancelled.cancel(NOW + 1).unwrap();
		assert!(cancelled.drain_events().is_empty(), "a repeat cancel raises nothing");
		assert!(cancelled.expire(NOW + TTL_SECS + 1).is_err());
		assert!(cancelled.record_vote(roster[1], VoteDecision::Approve, NOW).is_err());

		let mut expired = opened(&roster);
		// Refuses to cut the window short.
		assert!(matches!(expired.expire(NOW + 10), Err(DomainError::Conflict(_))));
		expired.expire(NOW + TTL_SECS).unwrap();
		assert_eq!(expired.state(), ConsiliumState::Expired);
		expired.expire(NOW + TTL_SECS + 99).unwrap();
		assert!(expired.cancel(NOW).is_err());
	}

	#[test]
	fn an_expired_consilium_can_never_execute() {
		let roster = owners(3);
		let mut c = opened(&roster);
		c.expire(NOW + TTL_SECS).unwrap();
		// Execution is reachable only from `approved`, and expiry is reachable only from
		// `open` — so no ordering of the two can produce an executed expired consilium.
		assert!(matches!(c.mark_executed(WithdrawalId::new(), NOW + TTL_SECS + 1), Err(DomainError::Conflict(_))));
		assert!(c.mark_execution_failed("late".into(), NOW).is_err());
	}

	#[test]
	fn execution_is_recorded_once_and_retries_are_no_ops() {
		let roster = owners(3);
		let mut c = opened(&roster);
		c.record_vote(roster[1], VoteDecision::Approve, NOW).unwrap();
		c.record_vote(roster[2], VoteDecision::Approve, NOW).unwrap();
		assert_eq!(c.state(), ConsiliumState::Approved);
		let payout = WithdrawalId::new();
		c.mark_executed(payout, NOW).unwrap();
		c.drain_events();
		// The deterministic id makes a retry name the same payout, so it is a no-op.
		c.mark_executed(payout, NOW + 5).unwrap();
		assert!(c.drain_events().is_empty());
		assert_eq!(c.executed_withdrawal_id(), Some(payout));
		// A different payout is a conflict, never a silent overwrite.
		assert!(matches!(c.mark_executed(WithdrawalId::new(), NOW + 6), Err(DomainError::Conflict(_))));
		// And a failure can no longer be recorded over a completed execution.
		assert!(c.mark_execution_failed("too late".into(), NOW + 7).is_err());
	}

	#[test]
	fn execution_failure_is_terminal_and_states_why() {
		let roster = owners(3);
		let mut c = opened(&roster);
		c.record_vote(roster[1], VoteDecision::Approve, NOW).unwrap();
		c.record_vote(roster[2], VoteDecision::Approve, NOW).unwrap();
		c.mark_execution_failed("payout exceeds the fund's available revenue".into(), NOW).unwrap();
		assert_eq!(c.state(), ConsiliumState::ExecutionFailed);
		assert_eq!(c.failure_reason(), Some("payout exceeds the fund's available revenue"));
		c.drain_events();
		c.mark_execution_failed("again".into(), NOW + 1).unwrap();
		assert!(c.drain_events().is_empty());
		// Nothing retries silently: an execution failure cannot become an execution.
		assert!(c.mark_executed(WithdrawalId::new(), NOW + 2).is_err());
	}

	#[test]
	fn the_version_moves_on_every_fact() {
		let roster = owners(3);
		let mut c = opened(&roster);
		let opened_at = c.version();
		c.record_vote(roster[1], VoteDecision::Approve, NOW).unwrap();
		assert!(c.version() > opened_at);
		let after_vote = c.version();
		c.record_vote(roster[2], VoteDecision::Approve, NOW).unwrap();
		// The carrying vote raises two facts: the vote and the verdict.
		assert_eq!(c.version(), after_vote + 2);
	}

	#[test]
	fn the_canonical_encoding_separates_fields_unambiguously() {
		let address = WalletAddress::parse(Network::Bep20, "0x52908400098527886E0F7030069857D2E4169EE7").unwrap();
		let amount = Usdt::parse_decimal("1").unwrap();
		// Two terms whose fields concatenate to the same string must NOT encode alike —
		// this is exactly what the length prefixes buy.
		let a = RevenuePayoutTerms::new(Network::Bep20, address.clone(), amount, "ab".to_owned()).unwrap();
		let b = RevenuePayoutTerms::new(Network::Bep20, address.clone(), amount, "a".to_owned()).unwrap();
		assert_ne!(a.canonical_bytes(), b.canonical_bytes());
		// The encoding is deterministic across calls, which is what makes the stored hash
		// re-verifiable at execution.
		assert_eq!(a.canonical_bytes(), a.canonical_bytes());
		// The amount is part of the digest, so editing it invalidates every approval.
		let dearer = RevenuePayoutTerms::new(Network::Bep20, address, Usdt::parse_decimal("2").unwrap(), "ab".to_owned()).unwrap();
		assert_ne!(a.canonical_bytes(), dearer.canonical_bytes());
	}

	#[test]
	fn terms_reject_a_mismatched_rail_and_an_unsendable_memo() {
		let bep20 = WalletAddress::parse(Network::Bep20, "0x52908400098527886E0F7030069857D2E4169EE7").unwrap();
		let amount = Usdt::parse_decimal("5").unwrap();
		assert!(RevenuePayoutTerms::new(Network::Trc20, bep20.clone(), amount, String::new()).is_err());
		assert!(RevenuePayoutTerms::new(Network::Bep20, bep20.clone(), amount, "x".repeat(MAX_MEMO_BYTES + 1)).is_err());
		assert!(RevenuePayoutTerms::new(Network::Bep20, bep20, amount, "line\nbreak".to_owned()).is_err());
	}

	#[test]
	fn states_and_decisions_round_trip_and_reject_junk() {
		for state in [
			ConsiliumState::Open,
			ConsiliumState::Approved,
			ConsiliumState::Rejected,
			ConsiliumState::Expired,
			ConsiliumState::Cancelled,
			ConsiliumState::Executed,
			ConsiliumState::ExecutionFailed,
		] {
			assert_eq!(ConsiliumState::parse(state.as_str()).unwrap(), state);
		}
		for decision in [VoteDecision::Pending, VoteDecision::Approve, VoteDecision::Reject] {
			assert_eq!(VoteDecision::parse(decision.as_str()).unwrap(), decision);
		}
		assert_eq!(ConsiliumKind::parse(ConsiliumKind::RevenuePayout.as_str()).unwrap(), ConsiliumKind::RevenuePayout);
		assert!(ConsiliumState::parse("done").is_err());
		assert!(VoteDecision::parse("maybe").is_err());
		assert!(ConsiliumKind::parse("owner_removal").is_err());
	}

	#[test]
	fn events_round_trip_through_json() {
		let roster = owners(3);
		let mut c = Consilium::open(ConsiliumId::new(), terms(), [0xab; 32], roster[0], &roster, NOW).unwrap();
		let event = c.drain_events().pop().unwrap();
		let json = serde_json::to_string(&event).unwrap();
		let back: ConsiliumEvent = serde_json::from_str(&json).unwrap();
		let ConsiliumEvent::Opened { payload_hash, threshold: t, .. } = back else {
			panic!("expected Opened")
		};
		assert_eq!(t, 2);
		assert_eq!(payload_hash, "ab".repeat(32));
	}
}
