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

/// One governance mail, addressed to a single person.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GovernanceMail {
	/// Asking an owner to approve a revenue payout. Carries secrets.
	PayoutApproval(PayoutApproval),
	/// Telling the audience how a consilium ended — over a payout or over a payment.
	PayoutOutcome(PayoutOutcome),
	/// Warning every owner that a token burned on failed code attempts.
	TokenBurned(PayoutOutcome),
	/// Asking ONE investor to consent to a payment out of their own claim. Carries secrets,
	/// and is the one kind addressed by identity rather than by seat.
	PaymentConsent(PaymentConsent),
	/// Asking an owner to approve a payment out of fund-owned money. Carries secrets.
	PaymentApproval(PaymentApproval),
}

impl GovernanceMail {
	/// The stored discriminant, matching the `consilium_mail.kind` CHECK.
	pub fn as_str(&self) -> &'static str {
		match self {
			Self::PayoutApproval(_) => "payout_approval",
			Self::PayoutOutcome(_) => "payout_outcome",
			Self::TokenBurned(_) => "token_burned",
			Self::PaymentConsent(_) => "payment_consent",
			Self::PaymentApproval(_) => "payment_approval",
		}
	}

	/// Whether this mail hands its recipient a token to answer with — the kinds whose
	/// delivery is what a seat's `notified` flag reports. An outcome or burn notice tells the
	/// recipient nothing about whether they can vote, so it flips nothing.
	pub fn carries_a_token(&self) -> bool {
		match self {
			Self::PayoutApproval(_) | Self::PaymentConsent(_) | Self::PaymentApproval(_) => true,
			Self::PayoutOutcome(_) | Self::TokenBurned(_) => false,
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
			Self::PaymentConsent(mail) => Self::PaymentConsent(PaymentConsent {
				approval_url: String::new(),
				code: String::new(),
				..mail.clone()
			}),
			Self::PaymentApproval(mail) => Self::PaymentApproval(PaymentApproval {
				approval_url: String::new(),
				code: String::new(),
				..mail.clone()
			}),
			Self::PayoutOutcome(_) | Self::TokenBurned(_) => self.clone(),
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
///
/// ONE shape for both subjects, additively — the wire's `PayoutOutcomeMail` is the same:
/// `network` + `address` describe a payout, `tier` + `source` + `destination` + `reason`
/// describe a payment, and the renderer switches on which pair is filled. The payment
/// fields default to empty so queue rows written before they existed still deserialize.
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
	#[serde(default)]
	pub tier: String,
	#[serde(default)]
	pub source: String,
	#[serde(default)]
	pub destination: String,
	#[serde(default)]
	pub reason: String,
}

/// The consent invitation to the ONE investor whose claim a payment spends.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PaymentConsent {
	pub payment_id: String,
	/// The subject's id IN THE IDENTITY PLANE. Concierge refuses the mail unless the user it
	/// is being asked to write to is this person — a caller that fans one consent out to a
	/// second mailbox has to contradict itself in the same message to do it.
	pub subject_user_id: String,
	pub initiator_email: String,
	pub tier: String,
	pub source: String,
	pub destination: String,
	pub amount: String,
	pub reason: String,
	pub payload_hash: String,
	pub expires_at: i64,
	/// Absolute URL of the consent page, carrying the opaque token.
	pub approval_url: String,
	/// The secret code. Cleared from the queue row on success.
	pub code: String,
}

/// The owner-facing approval invitation over a PAYMENT — the consilium counterpart of
/// [`PaymentConsent`]. Its own shape rather than a widened [`PayoutApproval`], because the
/// payout template opens with "a request to pay fund revenue out on-chain" and a payment
/// between two claims rendered through it would name the wrong claim and the wrong rail.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PaymentApproval {
	pub consilium_id: String,
	pub payment_id: String,
	pub initiator_email: String,
	pub tier: String,
	pub source: String,
	pub destination: String,
	pub amount: String,
	pub reason: String,
	pub payload_hash: String,
	pub threshold: u32,
	pub owner_count: u32,
	pub expires_at: i64,
	/// Absolute URL of the approval page, carrying the opaque token.
	pub approval_url: String,
	/// The secret code. Cleared from the queue row on success.
	pub code: String,
}

/// Why a mail was not taken — split by what the worker should do about it.
///
/// The identity plane rate-limits governance mail per recipient and answers
/// `RESOURCE_EXHAUSTED`; it can also simply be unreachable. Neither says anything about
/// the message, so neither may spend one of the message's attempts: a recipient who is
/// throttled ten times in five minutes would otherwise lose their approval token for good
/// while the mechanism reported a delivery failure that never happened.
#[derive(Debug)]
pub enum MailDeliveryError {
	/// Try again later, charging nothing: the relay is throttling this recipient or is down.
	Deferred(String),
	/// The relay refused this message, or the transport failed in a way a retry may fix;
	/// each such answer costs an attempt.
	Failed(String),
}

impl core::fmt::Display for MailDeliveryError {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		match self {
			Self::Deferred(why) | Self::Failed(why) => f.write_str(why),
		}
	}
}

impl From<MailDeliveryError> for DomainError {
	fn from(err: MailDeliveryError) -> Self {
		DomainError::Repository(err.to_string())
	}
}

/// The driven port: hand one mail to the identity plane's mailer.
///
/// `dedupe_key` makes the call idempotent on concierge's side, so the retrying worker
/// behind this port may redeliver freely without sending twice. `concierge_user_id` is the
/// recipient's id **in the plane that owns identities** — the address is resolved there,
/// which is what stops the money plane from redirecting a governance mail.
#[async_trait]
pub trait GovernanceMailer: Send + Sync {
	async fn send(&self, concierge_user_id: uuid::Uuid, dedupe_key: &str, mail: &GovernanceMail) -> Result<(), MailDeliveryError>;
}
