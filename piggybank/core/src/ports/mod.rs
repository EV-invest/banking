//! Driven ports — the outbound interfaces the application depends on, implemented
//! by `infrastructure`. The hexagonal "domain/port" layer over the generic DDD
//! building blocks in [`domain::architecture`].
//!
//! [`UserRepository`] is the first leaf port: it ties the [`User`] aggregate to its
//! Postgres persistence and the narrow read side ([`Reader`]). Methods are
//! use-case-shaped and each is internally atomic — the aggregate's drained events
//! are written to the event log in the same transaction as the state change (the
//! ACID point), so callers never juggle a transaction across the port boundary.
//!
//! The money plane adds the fund-currency ports — [`SubscriptionRepository`],
//! [`RedemptionRepository`], [`WithdrawalRepository`], and [`NavRepository`] — each
//! the aggregate's atomic, row-locked persistence, plus [`Ledger`], the TigerBeetle
//! [`Gateway`](domain::architecture::Gateway), which by construction cannot enrol in
//! a Postgres `UnitOfWork` (money is written last, in the relay).

pub mod custody;
pub mod deposit_addresses;
pub mod ledger;
pub mod nav;
pub mod positions;
pub mod redemptions;
pub mod subscriptions;
pub mod withdrawals;

use async_trait::async_trait;
pub use custody::{BroadcastRequest, Custody, CustodyError};
pub use deposit_addresses::DepositAddresses;
use domain::{
	architecture::{Reader, Repository},
	auth::AuthSubject,
	error::DomainError,
	users::{Email, User, UserId},
};
pub use ledger::{CompletionKind, Ledger, LedgerBalance, LedgerError, LedgerTransfer, PendingCompletion};
pub use nav::{NavRepository, Valuation};
pub use positions::{FundPosition, FundPositionReader};
pub use redemptions::RedemptionRepository;
pub use subscriptions::SubscriptionRepository;
pub use withdrawals::WithdrawalRepository;

/// Persistence + read port for the [`User`] aggregate.
#[async_trait]
pub trait UserRepository: Repository<Aggregate = User> + Reader<Aggregate = User> {
	/// Find a user by hub id.
	async fn find_by_id(&self, id: UserId) -> Result<Option<User>, DomainError>;

	/// Upsert by the immutable [`AuthSubject`] at sign-in: create (raising
	/// `Provisioned`) or refresh the email (raising `EmailChanged`), atomically with
	/// the event log. Idempotent for concurrent first-logins. Returns the current
	/// aggregate.
	async fn provision(&self, subject: AuthSubject, email: Email, email_verified: bool) -> Result<User, DomainError>;

	/// Persist a mutated aggregate — its new state and its drained events to the
	/// event log — in one transaction.
	async fn save(&self, user: &mut User) -> Result<(), DomainError>;
}
