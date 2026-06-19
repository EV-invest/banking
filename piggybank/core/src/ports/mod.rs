//! Driven ports — the outbound interfaces the application depends on, implemented
//! by `infrastructure`. The hexagonal "domain/port" layer over the generic DDD
//! building blocks in [`domain::architecture`](domain::architecture).
//!
//! [`UserRepository`] is the first leaf port: it ties the [`User`] aggregate to its
//! Postgres persistence and the narrow read side ([`Reader`]). Methods are
//! use-case-shaped and each is internally atomic — the aggregate's drained events
//! are written to the event log in the same transaction as the state change (the
//! ACID point), so callers never juggle a transaction across the port boundary.
//!
//! The TigerBeetle `Ledger: Gateway` port lands with the first money-moving slice;
//! today balances are read inline (one id-map lookup + a live `lookup_accounts`),
//! so no gateway port is introduced before it earns its keep.

use async_trait::async_trait;
use domain::{
	architecture::{Reader, Repository},
	auth::AuthSubject,
	error::DomainError,
	users::{Email, User, UserId},
};

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
