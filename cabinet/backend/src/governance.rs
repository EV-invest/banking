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
