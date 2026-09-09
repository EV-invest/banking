//! The one seam over which the money plane asks the identity plane to send a governance
//! mail.
//!
//! WHY NOT SEND IT HERE. Concierge owns the only mailer on the platform — the transport,
//! the delivery queue, the backoff and the daily budget — and the house rule forbids
//! hand-wiring a second vendor SDK. So banking hands over a **typed** payload and concierge
//! renders it. Never rendered HTML: a compromised money plane must not become a way to put
//! arbitrary markup into an owner's mailbox.
//!
//! WHY THE PAYLOADS ARE OUR OWN TYPES. These structs are the shape of a queue row, not the
//! wire. The adapter maps them onto the generated `concierge.v1` messages at the boundary,
//! so the queue's JSON is stable across a contract regeneration and nothing here pretends
//! to be a protobuf.

use async_trait::async_trait;
use domain::error::DomainError;
use serde::{Deserialize, Serialize};

/// One governance mail, addressed to a single owner.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GovernanceMail {
	/// Asking an owner to approve a payout. The only variant carrying secrets.
	PayoutApproval(PayoutApproval),
	/// Telling the owners how a consilium ended.
	PayoutOutcome(PayoutOutcome),
	/// Warning every owner that a token burned on failed code attempts.
	TokenBurned(PayoutOutcome),
}

impl GovernanceMail {
	/// The stored discriminant, matching the `consilium_mail.kind` CHECK.
	pub fn as_str(&self) -> &'static str {
		match self {
			Self::PayoutApproval(_) => "payout_approval",
			Self::PayoutOutcome(_) => "payout_outcome",
			Self::TokenBurned(_) => "token_burned",
		}
	}

	/// The same mail with its secrets removed, written back over the queue row once the
	/// message has been handed to concierge. The plaintext code exists for exactly as long
	/// as it takes to deliver it and no longer.
	pub fn redacted(&self) -> Self {
		match self {
			Self::PayoutApproval(mail) => Self::PayoutApproval(PayoutApproval {
				approval_url: String::new(),
				code: String::new(),
				..mail.clone()
			}),
			other => other.clone(),
		}
	}
}

/// The approval invitation. Everything an owner needs to judge the request before typing
/// the code — and the address in FULL, because a truncated one in an approval mail is an
/// invitation to approve the wrong wallet.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PayoutApproval {
	pub consilium_id: String,
	pub initiator_email: String,
	pub network: String,
	pub address: String,
	pub amount: String,
	pub memo: String,
	pub payload_hash: String,
	pub threshold: u32,
	pub owner_count: u32,
	pub expires_at: i64,
	/// Absolute URL of the approval page, carrying the opaque token.
	pub approval_url: String,
	/// The secret code. Cleared from the queue row on success.
	pub code: String,
}

/// How a consilium ended, or why a token burned.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PayoutOutcome {
	pub consilium_id: String,
	/// APPROVED / REJECTED / EXPIRED / CANCELLED / EXECUTED / EXECUTION_FAILED, or
	/// TOKEN_BURNED for the burn notice.
	pub outcome: String,
	pub network: String,
	pub address: String,
	pub amount: String,
	pub detail: String,
}

/// The driven port: hand one mail to the identity plane's mailer.
///
/// `dedupe_key` makes the call idempotent on concierge's side, so the retrying worker
/// behind this port may redeliver freely without sending twice. `concierge_user_id` is the
/// recipient's id **in the plane that owns identities** — the address is resolved there,
/// which is what stops the money plane from redirecting a governance mail.
#[async_trait]
pub trait GovernanceMailer: Send + Sync {
	async fn send(&self, concierge_user_id: uuid::Uuid, dedupe_key: &str, mail: &GovernanceMail) -> Result<(), DomainError>;
}
