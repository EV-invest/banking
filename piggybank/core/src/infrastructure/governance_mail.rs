//! Driven adapter for the [`GovernanceMailer`] port — concierge's
//! `MailRelayService.SendGovernanceMail`.
//!
//! # THE PIN PROBLEM — READ BEFORE TOUCHING THIS FILE
//!
//! `governance.proto` (which declares `MailRelayService`) lives in the concierge repo and
//! has **not merged yet**. Banking pins `evconcierge_contracts` to a git rev that does not
//! contain it, so `evconcierge_contracts::concierge::v1::mail_relay_service_client` does
//! **not exist** in this build. Fabricating the wire types locally would compile and then
//! diverge silently the moment the real proto changed a field number, so this file does not
//! do that.
//!
//! Instead the whole transport lives behind the off-by-default `concierge_governance_mail`
//! feature, and the default build wires **no** mailer at all. The queue still fills — every
//! mail is written in the same transaction as the consilium fact it announces, so nothing is
//! lost — it simply is not drained until the seam is live.
//!
//! **TODO(consilium): once the concierge governance PR merges, bump the
//! `evconcierge_contracts` (and `evconcierge_auth`) rev in the workspace `Cargo.toml` to the
//! rev that ships `contracts/proto/concierge/v1/governance.proto`, then make
//! `concierge_governance_mail` a default feature of `piggybank-core` and delete this
//! paragraph.** Until that lands, an operator sees the backlog through the boot warning and
//! the mailer's own logs, and no owner is ever emailed a payout approval.

use domain::error::DomainError;

use crate::ports::governance_mail::GovernanceMail;

/// The outcome vocabulary the concierge templates switch on. Kept beside the mapping so the
/// two planes' understanding of "what kind of mail is this" stays in one place.
pub fn mail_kind_str(mail: &GovernanceMail) -> &'static str {
	match mail {
		GovernanceMail::PayoutApproval(_) => "PAYOUT_APPROVAL",
		GovernanceMail::PayoutOutcome(_) => "PAYOUT_OUTCOME",
		GovernanceMail::TokenBurned(_) => "APPROVAL_TOKEN_BURNED",
	}
}

/// Whether the governance-mail seam is compiled in at all. `false` on the default build —
/// see the module docs.
pub const fn is_wired() -> bool {
	cfg!(feature = "concierge_governance_mail")
}

/// The gRPC adapter. Compiled only once the pin carries `governance.proto`.
///
/// The mapping below is written against the frozen contract in the concierge worktree
/// (`contracts/proto/concierge/v1/governance.proto`); it is deliberately explicit, field by
/// field, so that a contract change becomes a compile error here rather than a mail that
/// renders wrong.
#[cfg(feature = "concierge_governance_mail")]
pub mod wired {
	use async_trait::async_trait;
	use domain::error::DomainError;
	use evconcierge_contracts::concierge::v1::{GovernanceMailKind, PayoutApprovalMail, PayoutOutcomeMail, SendGovernanceMailRequest, mail_relay_service_client::MailRelayServiceClient};
	use tonic::{Request, metadata::MetadataValue, transport::Channel};
	use uuid::Uuid;

	use crate::ports::governance_mail::{GovernanceMail, GovernanceMailer};

	/// Calls concierge's mail relay, authenticated with the shared banking↔concierge service
	/// secret — the same `BRIDGE_SERVICE_TOKEN` the one-way lifecycle bridge presents, on the
	/// same channel, because it is the same trust relationship.
	pub struct ConciergeGovernanceMailer {
		channel: Channel,
		service_token: String,
	}

	impl ConciergeGovernanceMailer {
		pub fn new(channel: Channel, service_token: String) -> Self {
			Self { channel, service_token }
		}
	}

	#[async_trait]
	impl GovernanceMailer for ConciergeGovernanceMailer {
		async fn send(&self, concierge_user_id: Uuid, dedupe_key: &str, mail: &GovernanceMail) -> Result<(), DomainError> {
			let mut payload = SendGovernanceMailRequest {
				kind: GovernanceMailKind::Unspecified as i32,
				user_id: concierge_user_id.to_string(),
				dedupe_key: dedupe_key.to_owned(),
				payout_approval: None,
				payout_outcome: None,
			};
			match mail {
				GovernanceMail::PayoutApproval(approval) => {
					payload.kind = GovernanceMailKind::PayoutApproval as i32;
					payload.payout_approval = Some(PayoutApprovalMail {
						consilium_id: approval.consilium_id.clone(),
						initiator_email: approval.initiator_email.clone(),
						network: approval.network.clone(),
						address: approval.address.clone(),
						amount: approval.amount.clone(),
						memo: approval.memo.clone(),
						payload_hash: approval.payload_hash.clone(),
						threshold: approval.threshold,
						owner_count: approval.owner_count,
						expires_at: approval.expires_at,
						approval_url: approval.approval_url.clone(),
						code: approval.code.clone(),
					});
				}
				// The burn notice shares `payout_outcome`: the contract declares a distinct
				// KIND for it but no payload message of its own, so the outcome shape (with
				// `outcome = TOKEN_BURNED`) is the only carrier available.
				GovernanceMail::PayoutOutcome(outcome) | GovernanceMail::TokenBurned(outcome) => {
					payload.kind = if matches!(mail, GovernanceMail::TokenBurned(_)) {
						GovernanceMailKind::ApprovalTokenBurned as i32
					} else {
						GovernanceMailKind::PayoutOutcome as i32
					};
					payload.payout_outcome = Some(PayoutOutcomeMail {
						consilium_id: outcome.consilium_id.clone(),
						outcome: outcome.outcome.clone(),
						network: outcome.network.clone(),
						address: outcome.address.clone(),
						amount: outcome.amount.clone(),
						detail: outcome.detail.clone(),
					});
				}
			}
			let mut request = Request::new(payload);
			let token: MetadataValue<_> = format!("Bearer {}", self.service_token)
				.parse()
				.map_err(|_| DomainError::Repository("malformed governance mail service token".into()))?;
			request.metadata_mut().insert("authorization", token);
			MailRelayServiceClient::new(self.channel.clone())
				.send_governance_mail(request)
				.await
				.map(|_| ())
				.map_err(|status| DomainError::Repository(format!("governance mail relay: {status}")))
		}
	}
}

/// The reason a default build has no mailer, in one place so the boot log and the worker
/// agree on the wording.
pub fn unwired_reason() -> DomainError {
	DomainError::Repository("the concierge governance-mail seam is not compiled in (evconcierge_contracts predates governance.proto)".into())
}

/// Log the seam's state once at boot. An operator must never discover from an owner that no
/// approval mail went out, so an unwired build says so loudly and says what fixes it.
pub fn announce_wiring(pending: i64) {
	if is_wired() {
		tracing::info!("consilium: governance mail relay wired");
	} else {
		tracing::error!(
			pending,
			"consilium: governance mail is NOT wired — approval mails are being QUEUED BUT NOT SENT. \
			 No owner can receive an approval link, so no payout consilium can reach quorum. \
			 Fix: bump evconcierge_contracts to the rev shipping concierge/v1/governance.proto and \
			 enable the piggybank-core `concierge_governance_mail` feature."
		);
	}
}
