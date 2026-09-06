//! `consilium` context — the two gRPC surfaces of multi-owner payout authorization.
//!
//! [`ConsiliumSvc`] sits **behind** the user-auth layer and is Owner-only: opening, reading
//! and withdrawing a request are things a signed-in owner does.
//!
//! [`ConsiliumApprovalSvc`] is mounted **outside** it, exactly as the auth service's own
//! routes are. The emailed token IS the credential there, and the owner clicking a link in
//! their mailbox may well not be signed in — requiring a session would make the mail
//! useless. Everything that surface returns is deliberately narrow, and every unusable token
//! leaves by the same door.
//!
//! `Result<_, Status>` is tonic's mandated handler signature; `Status` is a large type we
//! don't control, so the large-err lint does not apply in this module.
#![allow(clippy::result_large_err)]

use domain::{
	authz::Permission,
	consilium::{ConsiliumState, RevenuePayoutTerms, VoteDecision},
	error::DomainError,
	money::{Network, Usdt, WalletAddress},
};
use evbanking_contracts::banking::v1::{self as pb, consilium_approval_service_server::ConsiliumApprovalService, consilium_service_server::ConsiliumService};
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::{
	AppState,
	application::consilium as consilium_app,
	ports::consilium::{ConsiliumView, InvitationView, VoteAudit},
	services::support::{caller_id, map_err, require_permission, unix_now},
};

/// The default page size for the governance history.
const DEFAULT_LIST_LIMIT: u32 = 50;
const MAX_LIST_LIMIT: u32 = 200;

#[derive(Clone)]
pub struct ConsiliumSvc {
	pub state: AppState,
}

impl ConsiliumSvc {
	pub fn new(state: AppState) -> Self {
		Self { state }
	}
}

#[derive(Clone)]
pub struct ConsiliumApprovalSvc {
	pub state: AppState,
}

impl ConsiliumApprovalSvc {
	pub fn new(state: AppState) -> Self {
		Self { state }
	}
}

impl AppState {
	fn consilium_ports(&self) -> consilium_app::ConsiliumPorts<'_> {
		consilium_app::ConsiliumPorts {
			consilia: self.consilia.as_ref(),
			withdrawals: self.withdrawals.as_ref(),
			ledger: self.ledger.as_ref(),
			custody: self.custody.as_ref(),
			relay: &self.relay_notify,
			configured: &self.configured_networks,
			approval_url_base: &self.consilium_approval_url_base,
			governance_mail_wired: crate::infrastructure::governance_mail::is_wired(),
		}
	}
}

fn parse_consilium_id(raw: &str) -> Result<domain::consilium::ConsiliumId, Status> {
	Uuid::parse_str(raw)
		.map(domain::consilium::ConsiliumId::from_raw)
		.map_err(|_| Status::invalid_argument("invalid consilium id"))
}

/// The terms, validated into their domain form. Shape errors surface here rather than after
/// three owners have approved something that could never have shipped.
fn parse_terms(terms: Option<pb::RevenuePayoutTerms>) -> Result<RevenuePayoutTerms, Status> {
	let terms = terms.ok_or_else(|| Status::invalid_argument("terms are required"))?;
	let network = Network::parse(&terms.network).map_err(map_err)?;
	let address = WalletAddress::parse(network, &terms.address).map_err(map_err)?;
	let amount = Usdt::parse_decimal(&terms.amount).map_err(map_err)?;
	RevenuePayoutTerms::new(network, address, amount, terms.memo).map_err(map_err)
}

fn state_to_proto(state: ConsiliumState) -> i32 {
	let mapped = match state {
		ConsiliumState::Open => pb::ConsiliumState::Open,
		ConsiliumState::Approved => pb::ConsiliumState::Approved,
		ConsiliumState::Rejected => pb::ConsiliumState::Rejected,
		ConsiliumState::Expired => pb::ConsiliumState::Expired,
		ConsiliumState::Cancelled => pb::ConsiliumState::Cancelled,
		ConsiliumState::Executed => pb::ConsiliumState::Executed,
		ConsiliumState::ExecutionFailed => pb::ConsiliumState::ExecutionFailed,
	};
	mapped as i32
}

fn decision_to_proto(decision: VoteDecision) -> i32 {
	let mapped = match decision {
		VoteDecision::Pending => pb::VoteDecision::Pending,
		VoteDecision::Approve => pb::VoteDecision::Approve,
		VoteDecision::Reject => pb::VoteDecision::Reject,
	};
	mapped as i32
}

/// `PENDING` is not an answer, so it is refused rather than silently recorded as one.
fn decision_from_proto(raw: i32) -> Result<VoteDecision, Status> {
	match pb::VoteDecision::try_from(raw) {
		Ok(pb::VoteDecision::Approve) => Ok(VoteDecision::Approve),
		Ok(pb::VoteDecision::Reject) => Ok(VoteDecision::Reject),
		_ => Err(Status::invalid_argument("decision must be approve or reject")),
	}
}

fn terms_to_proto(terms: &RevenuePayoutTerms) -> pb::RevenuePayoutTerms {
	pb::RevenuePayoutTerms {
		network: terms.network.as_str().to_owned(),
		// In FULL, never truncated: a shortened address in an approval flow is an
		// invitation to approve the wrong wallet.
		address: terms.address.as_str().to_owned(),
		amount: terms.amount.to_decimal_string(),
		memo: terms.memo.clone(),
	}
}

/// `alice@example.com` → `a***@example.com`. Used on every surface a non-owner can reach:
/// an emailed owner needs to recognise their own address, not learn anyone else's.
fn mask_email(email: &str) -> String {
	let Some((local, domain)) = email.split_once('@') else {
		return String::new();
	};
	match local.chars().next() {
		Some(first) => format!("{first}***@{domain}"),
		None => format!("***@{domain}"),
	}
}

fn consilium_to_proto(view: &ConsiliumView) -> pb::Consilium {
	let c = &view.consilium;
	pb::Consilium {
		id: c.id().to_string(),
		state: state_to_proto(c.state()),
		revenue_payout: Some(terms_to_proto(c.terms())),
		payload_hash: c.payload_hash_hex(),
		initiator_user_id: c.initiator().to_string(),
		initiator_email: view.initiator_email.clone(),
		owner_count: c.owner_count(),
		threshold: c.threshold(),
		// The counted tally, not the raw one: a vote from a seat that has since been lost
		// is shown against its voter but never added up here.
		approvals: c.approvals(),
		rejections: c.rejections(),
		voters: view
			.voters
			.iter()
			.map(|voter| pb::ConsiliumVoter {
				user_id: voter.user_id.to_string(),
				email: voter.email.clone(),
				decision: decision_to_proto(voter.decision),
				decided_at: voter.decided_at,
				notified: voter.notified,
			})
			.collect(),
		created_at: c.created_at(),
		expires_at: c.expires_at(),
		decided_at: c.decided_at().unwrap_or_default(),
		executed_withdrawal_id: c.executed_withdrawal_id().map(|id| id.to_string()).unwrap_or_default(),
		failure_reason: c.failure_reason().unwrap_or_default().to_owned(),
		version: c.version(),
	}
}

fn invitation_to_proto(view: &InvitationView) -> pb::ConsiliumInvitation {
	pb::ConsiliumInvitation {
		consilium_id: view.consilium_id.to_string(),
		state: state_to_proto(view.state),
		revenue_payout: Some(terms_to_proto(&view.terms)),
		payload_hash: view.payload_hash.clone(),
		initiator_email: mask_email(&view.initiator_email),
		voter_email: mask_email(&view.voter_email),
		threshold: view.threshold,
		approvals: view.approvals,
		owner_count: view.owner_count,
		created_at: view.created_at,
		expires_at: view.expires_at,
		decision: decision_to_proto(view.decision),
		attempts_remaining: view.attempts_remaining,
	}
}

/// The single answer every unusable token gets. Unknown, expired, spent, burned and closed
/// are one response with one message, so the surface cannot be probed for live tokens.
fn approval_err(err: DomainError) -> Status {
	match err {
		DomainError::NotFound { .. } => Status::not_found("invitation"),
		other => map_err(other),
	}
}

#[tonic::async_trait]
impl ConsiliumService for ConsiliumSvc {
	async fn open_revenue_payout(&self, request: Request<pb::OpenRevenuePayoutRequest>) -> Result<Response<pb::Consilium>, Status> {
		// Owner-only, through the same matrix the direct payout RPC uses. `RevenuePayout` is
		// the capability that moves company money outward; opening a consilium is the first
		// step of exactly that act.
		require_permission(&self.state, &request, Permission::RevenuePayout).await?;
		let initiator = caller_id(&request)?;
		let terms = parse_terms(request.into_inner().terms)?;
		let view = consilium_app::open_revenue_payout(&self.state.consilium_ports(), initiator, terms, unix_now())
			.await
			.map_err(map_err)?;
		// WARN on success on purpose: a request to move company money out is worth an audit
		// line that stands out, the same way the payout itself is.
		tracing::warn!(
			consilium_id = %view.consilium.id(),
			initiator = %initiator,
			network = %view.consilium.terms().network,
			address = %view.consilium.terms().address.as_str(),
			amount = %view.consilium.terms().amount,
			threshold = view.consilium.threshold(),
			owner_count = view.consilium.owner_count(),
			"opened a revenue-payout consilium"
		);
		Ok(Response::new(consilium_to_proto(&view)))
	}

	async fn cancel_consilium(&self, request: Request<pb::CancelConsiliumRequest>) -> Result<Response<pb::Consilium>, Status> {
		require_permission(&self.state, &request, Permission::RevenuePayout).await?;
		let caller = caller_id(&request)?;
		let id = parse_consilium_id(&request.get_ref().consilium_id)?;
		let view = consilium_app::cancel(self.state.consilia.as_ref(), id, caller, unix_now()).await.map_err(map_err)?;
		Ok(Response::new(consilium_to_proto(&view)))
	}

	async fn get_consilium(&self, request: Request<pb::GetConsiliumRequest>) -> Result<Response<pb::Consilium>, Status> {
		require_permission(&self.state, &request, Permission::RevenuePayout).await?;
		let id = parse_consilium_id(&request.get_ref().consilium_id)?;
		let view = consilium_app::find(self.state.consilia.as_ref(), id).await.map_err(map_err)?;
		Ok(Response::new(consilium_to_proto(&view)))
	}

	async fn list_consilia(&self, request: Request<pb::ListConsiliaRequest>) -> Result<Response<pb::ConsiliumList>, Status> {
		require_permission(&self.state, &request, Permission::RevenuePayout).await?;
		let requested = request.get_ref().limit;
		let limit = if requested == 0 { DEFAULT_LIST_LIMIT } else { requested.min(MAX_LIST_LIMIT) };
		let views = consilium_app::list(self.state.consilia.as_ref(), i64::from(limit)).await.map_err(map_err)?;
		Ok(Response::new(pb::ConsiliumList {
			items: views.iter().map(consilium_to_proto).collect(),
		}))
	}
}

#[tonic::async_trait]
impl ConsiliumApprovalService for ConsiliumApprovalSvc {
	async fn get_invitation(&self, request: Request<pb::GetInvitationRequest>) -> Result<Response<pb::ConsiliumInvitation>, Status> {
		// STRICTLY side-effect free. Mail scanners fetch every URL in a message; if this
		// counted an attempt or spent the token, a corporate gateway would destroy an
		// owner's vote before they ever read the mail.
		let view = consilium_app::invitation(self.state.consilia.as_ref(), &request.get_ref().token, unix_now())
			.await
			.map_err(approval_err)?;
		Ok(Response::new(invitation_to_proto(&view)))
	}

	async fn submit_decision(&self, request: Request<pb::SubmitDecisionRequest>) -> Result<Response<pb::SubmitDecisionResponse>, Status> {
		let req = request.into_inner();
		let decision = decision_from_proto(req.decision)?;
		let audit = VoteAudit {
			client_ip: req.client_ip,
			user_agent: req.user_agent,
		};
		let now = unix_now();
		let outcome = consilium_app::submit_decision(self.state.consilia.as_ref(), &req.token, &req.code, decision, &audit, now)
			.await
			.map_err(approval_err)?;
		if outcome.approved {
			// The vote is already committed, so an execution failure must not fail this
			// response — it is recorded on the consilium and the sweeper retries an
			// approval whose payout never got created.
			let consilium_id = outcome.invitation.consilium_id;
			if let Err(err) = consilium_app::execute(&self.state.consilium_ports(), consilium_id, now).await {
				tracing::error!(%consilium_id, "consilium: reached quorum but the payout could not be created: {err}");
			}
		}
		Ok(Response::new(pb::SubmitDecisionResponse {
			invitation: Some(invitation_to_proto(&outcome.invitation)),
			decided: outcome.decided,
		}))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn an_email_is_masked_to_its_first_letter_and_domain() {
		// What an emailed owner needs is to recognise their own seat, not to learn anyone
		// else's address.
		assert_eq!(mask_email("alice@example.com"), "a***@example.com");
		assert_eq!(mask_email("@example.com"), "***@example.com");
		// A value that is not an address discloses nothing at all rather than passing through.
		assert_eq!(mask_email("not-an-email"), "");
		assert_eq!(mask_email(""), "");
	}

	#[test]
	fn pending_is_not_an_acceptable_vote() {
		assert!(decision_from_proto(pb::VoteDecision::Approve as i32).is_ok());
		assert!(decision_from_proto(pb::VoteDecision::Reject as i32).is_ok());
		assert!(decision_from_proto(pb::VoteDecision::Pending as i32).is_err());
		assert!(decision_from_proto(pb::VoteDecision::Unspecified as i32).is_err());
		assert!(decision_from_proto(99).is_err());
	}

	#[test]
	fn every_unusable_token_produces_one_identical_answer() {
		// Unknown, expired, spent and burned all arrive here as the same NotFound, and must
		// leave as the same Status — otherwise the surface can be probed for live tokens.
		let one = approval_err(crate::ports::consilium::invitation_not_found());
		let two = approval_err(DomainError::NotFound {
			entity: "invitation",
			id: "something else".to_owned(),
		});
		assert_eq!(one.code(), tonic::Code::NotFound);
		assert_eq!(one.message(), two.message());
	}
}
