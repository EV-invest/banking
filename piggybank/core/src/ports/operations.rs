//! The operation-feed port — the caller's activity timeline as one time-ordered
//! stream.
//!
//! A **query-side** port (CQRS read model), not a repository: it spans four
//! aggregates (deposits, withdrawals, subscriptions, redemptions) and owns none of
//! them. Nothing here writes, so there is no aggregate to hang a
//! [`Repository`](domain::architecture::Repository) marker on — like [`Deposits`](super::Deposits)
//! it is a plain driven port. The write side stays exactly where it was; this reads
//! the projections those aggregates already maintain.
//!
//! [`Operation`] is a sum type, not a bag of optional fields: a deposit genuinely has
//! no NAV and a subscription genuinely has no destination address, and modelling that
//! as `Option` everywhere would let an impossible row compile. Flattening to the wire's
//! discriminated message is the service layer's job.

use async_trait::async_trait;
use domain::{
	balance::ServiceId,
	error::DomainError,
	money::{Nav, Network, Shares, TxRef, Usdt, WalletAddress},
	redemptions::RedemptionState,
	users::UserId,
	withdrawals::WithdrawalState,
};
use uuid::Uuid;

/// The largest page [`OperationFeed::list_by_user`] will return, whatever the caller
/// asks for. There is no cursor API: the timeline is a recent-activity surface, and
/// 200 rows is far beyond any account's realistic history at current scale.
pub const MAX_PAGE: u32 = 200;

/// The page size used when a caller does not specify one.
pub const DEFAULT_PAGE: u32 = 100;

#[async_trait]
pub trait OperationFeed: Send + Sync {
	/// The caller's operations, newest first, capped at `limit`. Returns the page
	/// alongside whether more rows exist beyond it.
	async fn list_by_user(&self, user: UserId, limit: u32) -> Result<OperationPage, DomainError>;
}

/// One page of the timeline. `truncated` distinguishes "this is the whole history"
/// from "this is the newest `limit` of it" — the client can say so rather than imply
/// the account has done nothing else.
pub struct OperationPage {
	pub operations: Vec<Operation>,
	pub truncated: bool,
}

/// One event on the caller's timeline. The variants mirror the four user-initiated
/// money movements; each carries exactly the fields its kind has.
pub enum Operation {
	/// A confirmed on-chain credit. Terminal by construction — the row is written only
	/// once the watcher has the required confirmations, so there is no pending state.
	Deposit { tx_ref: TxRef, network: Network, amount: Usdt, created_at: i64 },
	/// An on-chain payout, at whatever point of the reserve → dispatch → settle saga it
	/// has reached. `tx_ref` is `None` until it settles.
	Withdrawal {
		id: Uuid,
		network: Network,
		address: WalletAddress,
		amount: Usdt,
		fee: Usdt,
		state: WithdrawalState,
		tx_ref: Option<TxRef>,
		created_at: i64,
	},
	/// An immutable mint: `cash` bought `units` at `nav`. No state — it either happened
	/// or the row does not exist.
	Subscription {
		id: Uuid,
		service: ServiceId,
		cash: Usdt,
		nav: Nav,
		units: Shares,
		created_at: i64,
	},
	/// A unit burn in the accept-and-queue saga. `units` are fixed at request; `nav` and
	/// `cash` are `None` until settle (settle-time pricing).
	Redemption {
		id: Uuid,
		service: ServiceId,
		units: Shares,
		nav: Option<Nav>,
		cash: Option<Usdt>,
		state: RedemptionState,
		created_at: i64,
	},
}

impl Operation {
	/// Unix seconds the hub recorded the operation — the sort key the four sources are
	/// merged on.
	pub fn created_at(&self) -> i64 {
		match self {
			Self::Deposit { created_at, .. } | Self::Withdrawal { created_at, .. } | Self::Subscription { created_at, .. } | Self::Redemption { created_at, .. } => *created_at,
		}
	}

	/// The wire discriminator.
	pub fn kind(&self) -> &'static str {
		match self {
			Self::Deposit { .. } => "deposit",
			Self::Withdrawal { .. } => "withdrawal",
			Self::Subscription { .. } => "subscription",
			Self::Redemption { .. } => "redemption",
		}
	}
}
