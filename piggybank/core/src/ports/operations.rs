//! The operation-feed port — the caller's activity timeline as one time-ordered
//! stream.
//!
//! A **query-side** port (CQRS read model), not a repository: it spans five
//! projections (deposits, withdrawals, subscriptions, redemptions, and the fund's fee
//! assessments) and owns none of them. Nothing here writes, so there is no aggregate to hang a
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
	money::{Nav, Network, Shares, TxRef, Usdt},
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

/// One event on the caller's timeline. Four variants mirror the user-initiated money
/// movements and the fifth is the fund charging its fee; each carries exactly the fields
/// its kind has.
pub enum Operation {
	/// A confirmed on-chain credit. Terminal by construction — the row is written only
	/// once the watcher has the required confirmations, so there is no pending state.
	Deposit { tx_ref: TxRef, network: Network, amount: Usdt, created_at: i64 },
	/// An on-chain payout, at whatever point of the reserve → dispatch → settle saga it
	/// has reached. `tx_ref` is `None` until it settles.
	Withdrawal {
		id: Uuid,
		network: Network,
		/// The destination **as recorded**, not a re-parsed [`WalletAddress`]. This row is
		/// displayed, never paid to — the address was validated when the withdrawal was
		/// requested, and re-asserting a chain's address rules at read time only means a
		/// rule that later tightens takes the caller's whole timeline (deposits included)
		/// down with the one row it rejects.
		address: String,
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
	/// The fund charging its own fee against this holding: `units` were clawed back at
	/// `nav`, worth `cash`, split into the `management` and `performance` legs.
	///
	/// It belongs on the timeline for a blunt reason — it is the only line item that
	/// reduces a holding without the investor doing anything. Every other variant here
	/// is something they initiated. Leaving it out would make units simply go missing.
	///
	/// No cash moves and there is no chain reference: the units go to the manager's fee
	/// account on the share ledger. `deferred` says part of the charge could not be
	/// collected and was carried as debt, which is why `cash` can be less than the
	/// management and performance legs add up to.
	FeeCharge {
		id: Uuid,
		service: ServiceId,
		units: Shares,
		nav: Nav,
		cash: Usdt,
		management: Usdt,
		performance: Usdt,
		deferred: bool,
		created_at: i64,
	},
}

impl Operation {
	/// Unix seconds the hub recorded the operation — the sort key the sources are
	/// merged on.
	pub fn created_at(&self) -> i64 {
		match self {
			Self::Deposit { created_at, .. }
			| Self::Withdrawal { created_at, .. }
			| Self::Subscription { created_at, .. }
			| Self::Redemption { created_at, .. }
			| Self::FeeCharge { created_at, .. } => *created_at,
		}
	}

	/// The wire discriminator.
	pub fn kind(&self) -> &'static str {
		match self {
			Self::Deposit { .. } => "deposit",
			Self::Withdrawal { .. } => "withdrawal",
			Self::Subscription { .. } => "subscription",
			Self::Redemption { .. } => "redemption",
			Self::FeeCharge { .. } => "fee",
		}
	}
}
