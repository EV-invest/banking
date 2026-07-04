//! The deposit-log port — the control-plane record behind the company-money
//! commands (seed capital, record an on-chain deposit).
//!
//! These are the aggregate-less standalone [`LedgerEvent`](domain::balance::LedgerEvent)
//! facts (see [`domain::balance`]) — there is no aggregate to hang a `Repository`
//! marker on, so like [`NavMarks`](super::NavMarks) this is a plain driven port. Each
//! method is its own atomic unit: the fact row (where one exists) and its outbox
//! event commit in one Postgres transaction; the relay moves the money afterwards
//! (Write-Last).

use async_trait::async_trait;
use domain::{
	balance::Party,
	error::DomainError,
	money::{Network, TxRef, Usdt},
	users::UserId,
};

#[async_trait]
pub trait Deposits: Send + Sync {
	/// Record the company's own capital seeded on `network` (`Dr WALLET / Cr FUND`)
	/// as an outbox event.
	async fn seed_capital(&self, network: Network, amount: Usdt) -> Result<(), DomainError>;

	/// Record an on-chain deposit, **idempotent by `tx_ref`**: the unique gate makes
	/// a second record of the same chain tx impossible, so the credit happens at most
	/// once even under concurrent recorders. Returns `true` if newly recorded,
	/// `false` for a duplicate.
	async fn record(&self, tx_ref: TxRef, party: Party, network: Network, amount: Usdt) -> Result<bool, DomainError>;

	/// The caller's credited on-chain deposits, newest first — a projection read of
	/// the idempotency-gate rows where `party_kind = 'user'`.
	async fn list_by_user(&self, user: UserId) -> Result<Vec<DepositRecord>, DomainError>;
}
/// A credited on-chain deposit, read back from the idempotency-gate rows — a
/// projection read model, not an aggregate.
pub struct DepositRecord {
	pub tx_ref: TxRef,
	pub network: Network,
	pub amount: Usdt,
	/// Unix seconds the hub recorded the credit.
	pub created_at: i64,
}
