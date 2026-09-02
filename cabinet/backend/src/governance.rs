//! The concierge OWNERSHIP plane — the owner roster, owner removals, and the live
//! governance feed — behind one seam.
//!
//! Ownership is `Role::Owner`, a concierge-owned fact, so only that plane may mutate it
//! and only that plane may authorize the mutation. Every call here therefore forwards the
//! **concierge** identity token; the banking money token has a different issuer and
//! `aud`, and the two-plane separation is what stops one plane's credential from
//! authorizing the other's decisions. The money plane runs its own, separate consilium
//! over its own mirrored roster (see [`crate::state::Grpc::list_consilia`]).
//!
//! # TODO(pin-bump): this module is a stub, and it is the ONLY one
//!
//! `evconcierge_contracts` is pinned in the workspace manifest to a git rev
//! (`34dc166016351917c7db6f6fe7f2da183b8316b8`) that predates
//! `contracts/proto/concierge/v1/governance.proto`, so `GovernanceService`,
//! `OwnerRemovalApprovalService` and their message types **do not exist** in the
//! generated crate yet. Every function below is `Unimplemented` (→ HTTP 501) until that
//! pin is bumped; nothing else in this crate names those types, so the rest of the
//! consilium surface — routes, CSRF, DTOs, error mapping, the socket — compiles, is
//! clippy-clean and is tested today.
//!
//! Bumping the pin means, here and nowhere else:
//! 1. add `fn concierge_channel(&self) -> Channel` to [`crate::state::Grpc`] (the field
//!    is private to that module) and build the two clients from it;
//! 2. fill in each function against the RPC named in its doc line, forwarding `token` as
//!    `authorization: Bearer` exactly as [`crate::state`] does — except the two public
//!    approval calls, which carry NO bearer: the emailed token IS the credential;
//! 3. write the `From<cc::…>` conversions for the governance DTOs in [`crate::dto`],
//!    re-checking every field against the proto, and mask both addresses on
//!    `OwnerRemovalInvitation` the way the banking invitation already does;
//! 4. make [`watch`] spawn one pump task per socket that forwards `GovernanceTick`s into
//!    the returned channel and exits when either end drops;
//! 5. delete the module-level `allow` below and this section.
// Every parameter of every stub is unused until step 2 above lands. The allow is scoped
// to this quarantined module and goes away with the TODO.
#![allow(unused_variables)]

use tokio::sync::mpsc;
use tonic::Status;

use crate::{dto, state::Grpc};

/// What a peer owner may answer on a removal. `Remove` carries the proposal; a single
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
}

/// One frame of the live governance feed: a REVISION and when it was produced, never a
/// tally and never a secret. The client refetches the authoritative snapshot when the
/// revision moves, so a stale or replayed frame can never render a wrong count.
pub struct Tick {
	pub revision: u64,
	pub at: i64,
	/// True for a keepalive, which repeats the current revision unchanged.
	pub heartbeat: bool,
}

/// `GovernanceService.ListOwners` — the current roster (Owner only).
pub async fn owners(grpc: &Grpc, token: &str) -> Result<dto::OwnerList, Status> {
	Err(unavailable())
}

/// `GovernanceService.ListOwnerRemovals` — newest first, including closed proposals.
pub async fn list_removals(grpc: &Grpc, token: &str, limit: u32) -> Result<dto::OwnerRemovalList, Status> {
	Err(unavailable())
}

/// `GovernanceService.OpenOwnerRemoval` — propose. The initiator casts no vote.
pub async fn open_removal(grpc: &Grpc, token: &str, target_user_id: &str, reason: &str) -> Result<dto::OwnerRemoval, Status> {
	Err(unavailable())
}

/// `GovernanceService.SubmitPeerVote` — a peer's live vote, cast while signed in. The
/// target and the initiator are both refused by the plane.
pub async fn peer_vote(grpc: &Grpc, token: &str, removal_id: &str, vote: RemovalVote) -> Result<dto::OwnerRemoval, Status> {
	Err(unavailable())
}

/// `GovernanceService.CancelOwnerRemoval` — withdraw a proposal the caller opened.
pub async fn cancel_removal(grpc: &Grpc, token: &str, removal_id: &str) -> Result<dto::OwnerRemoval, Status> {
	Err(unavailable())
}

/// `GovernanceService.ResignOwnership` — no consilium, but the floor of three still
/// applies. `confirm_email` is the typed confirmation the plane checks against the
/// caller's own address, so a resignation cannot be a stray click.
pub async fn resign(grpc: &Grpc, token: &str, confirm_email: &str) -> Result<dto::OwnerList, Status> {
	Err(unavailable())
}

/// `OwnerRemovalApprovalService.GetInvitation` — public, side-effect free, no bearer.
pub async fn removal_invitation(grpc: &Grpc, token: &str) -> Result<dto::OwnerRemovalInvitation, Status> {
	Err(unavailable())
}

/// `OwnerRemovalApprovalService.SubmitSelfDecision` — public, no bearer: the target
/// accepting or refusing their own removal with the code from the same mail. `client_ip`
/// and `user_agent` are AUDIT fields only.
pub async fn submit_self_decision(grpc: &Grpc, token: &str, code: &str, vote: RemovalVote, client_ip: &str, user_agent: &str) -> Result<dto::RemovalDecision, Status> {
	Err(unavailable())
}

/// `GovernanceService.WatchGovernance` — the live feed, one receiver per socket.
///
/// The pump task that will own the upstream stream is spawned per subscription and both
/// ends are dropped together, so a browser that disconnects takes its task with it rather
/// than leaking one per reconnect.
pub async fn watch(grpc: &Grpc, token: &str) -> Result<mpsc::Receiver<Tick>, Status> {
	Err(unavailable())
}

/// The one verdict every stub returns. `Unimplemented` and not `Unavailable`: the RPC is
/// genuinely absent from the deployed contract rather than momentarily unreachable, and
/// the 501 it maps to says exactly that to the browser.
fn unavailable() -> Status {
	Status::unimplemented("the ownership plane is not wired up in this build")
}

#[cfg(test)]
mod tests {
	use super::*;

	/// A vote arrives as a word from a browser. Anything outside the vocabulary must be
	/// refused rather than folded into a default — the default would itself be a vote.
	#[test]
	fn only_the_two_votes_parse() {
		assert!(matches!(RemovalVote::parse("remove"), Some(RemovalVote::Remove)));
		assert!(matches!(RemovalVote::parse("keep"), Some(RemovalVote::Keep)));
		assert!(RemovalVote::parse("REMOVE").is_none());
		assert!(RemovalVote::parse("").is_none());
		assert!(RemovalVote::parse("approve").is_none());
	}

	/// Until the pin moves, every ownership call must fail as NOT IMPLEMENTED — never as
	/// a silent success, and never as an empty roster a page would render as "no owners".
	#[tokio::test]
	async fn the_stubbed_plane_never_answers_with_data() {
		let grpc = Grpc::connect_lazy("http://127.0.0.1:1", "http://127.0.0.1:1", "http://127.0.0.1:1", None).expect("lazy channels");
		let Err(status) = owners(&grpc, "token").await else {
			panic!("the stub cannot answer with a roster");
		};
		assert_eq!(status.code(), tonic::Code::Unimplemented);
	}
}
