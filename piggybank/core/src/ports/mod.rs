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
pub use redemptions::{QueuedRedemption, RedemptionRepository};
pub use subscriptions::SubscriptionRepository;
use uuid::Uuid;
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

	/// Resolve the bridge-mirrored issuance slice by the user's CONCIERGE id (the handle the
	/// BFF carries from sign-in). `None` if no local mirror exists yet (the bridge `CREATED`
	/// has not been consumed). Read-only.
	async fn resolve_issuance_by_concierge_id(&self, concierge_id: Uuid) -> Result<Option<IssuanceTarget>, DomainError>;

	/// Resolve the same issuance slice by the hub user id — the refresh-time re-check.
	async fn resolve_issuance_by_banking_id(&self, banking_id: UserId) -> Result<Option<IssuanceTarget>, DomainError>;

	/// Persist a mutated aggregate — its new state and its drained events to the
	/// event log — in one transaction.
	async fn save(&self, user: &mut User) -> Result<(), DomainError>;
}
/// The minimal slice needed to mint a money-plane token for a user. The money plane never sees
/// Google's `sub`. Each field FOLDS the two revoke surfaces so EITHER invalidates a money token:
/// the cross-plane bridge columns (`frozen` from a concierge SUSPENDED, `concierge_token_version`
/// from a SESSIONS_REVOKED) AND banking's own aggregate columns (`status='disabled'`,
/// `token_version` from a banking-side "revoke all"). Read by raw SQL, not via the `User`
/// aggregate (which deliberately doesn't model the bridge columns).
pub struct IssuanceTarget {
	/// The hub user id — stamped as the minted token's `sub`.
	pub user_id: UserId,
	pub email: String,
	/// True when the user must be refused a money token — a concierge SUSPENDED (`frozen`) OR a
	/// banking-side disable (`status='disabled'`). Gates both issuance and refresh.
	pub disabled: bool,
	/// The effective revoke floor: the GREATER of concierge's revoke version (SESSIONS_REVOKED)
	/// and banking's own `token_version`. Minted as the token's `token_version` and re-checked on
	/// refresh, so EITHER a concierge or a banking "revoke all" invalidates the money family.
	pub token_version: u64,
}
