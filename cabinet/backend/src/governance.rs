//! The ownership plane's two browser-facing vocabularies, and the live feed's pump.
//!
//! The RPCs themselves live with every other plane's in [`crate::state`], and the wire
//! conversions in [`crate::dto`]. What is left here is what belongs to neither: the words
//! a browser is allowed to vote with, and the task that turns concierge's server-stream
//! into something a websocket can own.

use evconcierge_contracts::concierge::v1 as cc;
use tokio::sync::mpsc;
use tonic::Status;

use crate::state::Grpc;

/// How many ticks may sit between the upstream stream and the socket. Small on purpose:
/// a tick carries no payload the client needs, only the news that the revision moved, so
/// a slow browser should back-pressure the pump rather than accumulate a backlog of
/// revisions it will collapse into one refetch anyway.
const TICK_BUFFER: usize = 16;

/// What a peer owner may answer on a REMOVAL. `Remove` carries the proposal; a single
/// `Keep` ends the peer-unanimity path.
#[derive(Clone, Copy)]
pub enum RemovalVote {
	Remove,
	Keep,
}

impl RemovalVote {
	/// The browser's vocabulary. Anything else is a client that does not know what it is
	/// asking for, and is refused rather than defaulted — a defaulted vote is a vote.
	pub fn parse(raw: &str) -> Option<Self> {
		match raw {
			"remove" => Some(Self::Remove),
			"keep" => Some(Self::Keep),
			_ => None,
		}
	}

	pub fn wire(self) -> cc::RemovalVote {
		match self {
			Self::Remove => cc::RemovalVote::Remove,
			Self::Keep => cc::RemovalVote::Keep,
		}
	}
}

/// What a peer owner may answer on an ADMISSION.
///
/// Deliberately NOT [`RemovalVote`], mirroring the proto's own split: "remove/keep" and
/// "admit/reject" are different questions, and a surface that renders the wrong verb on a
/// governance vote is a surface that gets a vote cast by mistake. Sharing one enum here
/// would put a single rename between those two meanings.
#[derive(Clone, Copy)]
pub enum AdmissionVote {
	Admit,
	Reject,
}

impl AdmissionVote {
	pub fn parse(raw: &str) -> Option<Self> {
		match raw {
			"admit" => Some(Self::Admit),
			"reject" => Some(Self::Reject),
			_ => None,
		}
	}

	pub fn wire(self) -> cc::AdmissionVote {
		match self {
			Self::Admit => cc::AdmissionVote::Admit,
			Self::Reject => cc::AdmissionVote::Reject,
		}
	}
}

/// What an owner may answer on a USER proposal.
///
/// Neutral verbs, and a third enum rather than a reuse of either above — the proto makes
/// the same split for the same reason. Three kinds (suspension, reinstatement, admin
/// admission) share one vote, so a kind-specific verb would only mean something read
/// against the kind. `for`/`against` says which way the voter pushed; the SURFACE, which
/// knows the kind, is what renders that as "suspend" or "admit".
#[derive(Clone, Copy)]
pub enum ProposalVote {
	For,
	Against,
}

impl ProposalVote {
	pub fn parse(raw: &str) -> Option<Self> {
		match raw {
			"for" => Some(Self::For),
			"against" => Some(Self::Against),
			_ => None,
		}
	}

	pub fn wire(self) -> cc::ProposalVote {
		match self {
			Self::For => cc::ProposalVote::For,
			Self::Against => cc::ProposalVote::Against,
		}
	}
}

/// Which user proposals a listing asks for. Absent means every kind — the words are the
/// browser's, and an unrecognised one is refused rather than widened to "all", because a
/// filter that silently stops filtering shows the reader rows they did not ask for.
#[derive(Clone, Copy)]
pub enum ProposalKind {
	Suspension,
	Reinstatement,
	AdminAdmission,
}

impl ProposalKind {
	pub fn parse(raw: &str) -> Option<Self> {
		match raw {
			"suspension" => Some(Self::Suspension),
			"reinstatement" => Some(Self::Reinstatement),
			"admin_admission" => Some(Self::AdminAdmission),
			_ => None,
		}
	}

	pub fn wire(self) -> cc::UserProposalKind {
		match self {
			Self::Suspension => cc::UserProposalKind::Suspension,
			Self::Reinstatement => cc::UserProposalKind::Reinstatement,
			Self::AdminAdmission => cc::UserProposalKind::AdminAdmission,
		}
	}
}

/// One frame of the live governance feed: a REVISION and when it was produced, never a
/// tally and never a secret. One revision covers removals and admissions together, so a
/// single subscription follows the whole ownership surface, and the client refetches the
/// authoritative snapshot when it moves — a stale or replayed frame cannot render a wrong
/// count.
pub struct Tick {
	pub revision: u64,
	pub at: i64,
	/// True for a keepalive, which repeats the current revision unchanged.
	pub heartbeat: bool,
}

/// Subscribe to the ownership plane's revision feed.
///
/// The upstream stream is established before this returns, so a plane that cannot serve
/// the feed is refused at the handshake rather than by a socket that opens and then never
/// ticks. The pump task owns the stream and holds only the sender: when the socket drops
/// its receiver the next send fails and the task returns, so a browser that disconnects
/// takes its task and its subscription with it instead of leaking one per reconnect.
pub async fn watch(grpc: &Grpc, token: &str) -> Result<mpsc::Receiver<Tick>, Status> {
	let mut stream = grpc.watch_governance(token).await?;
	let (ticks, receiver) = mpsc::channel(TICK_BUFFER);

	tokio::spawn(async move {
		loop {
			match stream.message().await {
				Ok(Some(tick)) => {
					let tick = Tick {
						revision: tick.revision,
						at: tick.at,
						heartbeat: tick.heartbeat,
					};
					if ticks.send(tick).await.is_err() {
						break;
					}
				}
				Ok(None) => break,
				Err(status) => {
					tracing::warn!(code = ?status.code(), "governance feed ended with an error");
					break;
				}
			}
		}
	});

	Ok(receiver)
}

#[cfg(test)]
mod tests {
	use super::*;

	/// A vote arrives as a word from a browser. Anything outside the vocabulary must be
	/// refused rather than folded into a default — the default would itself be a vote.
	#[test]
	fn only_the_two_removal_votes_parse() {
		assert!(matches!(RemovalVote::parse("remove"), Some(RemovalVote::Remove)));
		assert!(matches!(RemovalVote::parse("keep"), Some(RemovalVote::Keep)));
		assert!(RemovalVote::parse("REMOVE").is_none());
		assert!(RemovalVote::parse("").is_none());
		assert!(RemovalVote::parse("approve").is_none());
	}

	/// The two vocabularies must not be interchangeable: an admission cannot be voted on
	/// with a removal's words, or a page rendering the wrong verb would still submit.
	#[test]
	fn the_two_vocabularies_do_not_overlap() {
		assert!(matches!(AdmissionVote::parse("admit"), Some(AdmissionVote::Admit)));
		assert!(matches!(AdmissionVote::parse("reject"), Some(AdmissionVote::Reject)));
		assert!(AdmissionVote::parse("remove").is_none());
		assert!(AdmissionVote::parse("keep").is_none());
		assert!(RemovalVote::parse("admit").is_none());
		assert!(RemovalVote::parse("reject").is_none());
	}

	/// The user-proposal vocabulary is a THIRD one, and must not accept either consilium's
	/// words. A page that submitted "remove" here would otherwise be answering a question
	/// nobody asked — the kinds share one vote precisely because the verb is decided on the
	/// surface, so the wire word has to stay neutral.
	#[test]
	fn the_user_proposal_vote_is_neutral_and_shares_no_word() {
		assert!(matches!(ProposalVote::parse("for"), Some(ProposalVote::For)));
		assert!(matches!(ProposalVote::parse("against"), Some(ProposalVote::Against)));
		for foreign in ["remove", "keep", "admit", "reject", "FOR", ""] {
			assert!(ProposalVote::parse(foreign).is_none(), "{foreign} must not parse as a proposal vote");
		}
	}

	/// An unrecognised kind is refused rather than widened to "every kind": a filter that
	/// silently stops filtering shows the reader rows they did not ask for.
	#[test]
	fn only_the_three_proposal_kinds_parse() {
		assert!(matches!(ProposalKind::parse("suspension"), Some(ProposalKind::Suspension)));
		assert!(matches!(ProposalKind::parse("reinstatement"), Some(ProposalKind::Reinstatement)));
		assert!(matches!(ProposalKind::parse("admin_admission"), Some(ProposalKind::AdminAdmission)));
		assert!(ProposalKind::parse("admission").is_none());
		assert!(ProposalKind::parse("").is_none());
	}

	/// The neutral words must land on the plane's own FOR/AGAINST and never on its
	/// `UNSPECIFIED`/`PENDING` — either of which the server could read as "no answer yet"
	/// while the owner believes they voted.
	#[test]
	fn every_proposal_vote_and_kind_maps_onto_its_own_proto_variant() {
		assert_eq!(ProposalVote::For.wire(), cc::ProposalVote::For);
		assert_eq!(ProposalVote::Against.wire(), cc::ProposalVote::Against);
		assert_eq!(ProposalKind::Suspension.wire(), cc::UserProposalKind::Suspension);
		assert_eq!(ProposalKind::Reinstatement.wire(), cc::UserProposalKind::Reinstatement);
		assert_eq!(ProposalKind::AdminAdmission.wire(), cc::UserProposalKind::AdminAdmission);
	}

	/// The words the browser sends must land on the proto variants they name, and not on
	/// the plane's `UNSPECIFIED`/`PENDING` — either of which a server could read as "no
	/// answer yet" while the owner believes they voted.
	#[test]
	fn every_vote_maps_onto_its_own_proto_variant() {
		assert_eq!(RemovalVote::Remove.wire(), cc::RemovalVote::Remove);
		assert_eq!(RemovalVote::Keep.wire(), cc::RemovalVote::Keep);
		assert_eq!(AdmissionVote::Admit.wire(), cc::AdmissionVote::Admit);
		assert_eq!(AdmissionVote::Reject.wire(), cc::AdmissionVote::Reject);
	}
}
