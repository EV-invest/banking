//! The TigerBeetle ledger gateway port.
//!
//! [`Ledger`] is a [`Gateway`] — the anti-corruption boundary to an external
//! transactional system that owns its own atomicity. The kernel gives `Gateway` no
//! `UnitOfWork` accessor, so "the ledger cannot join a Postgres transaction" is a
//! compile-discoverable fact: money is written **last**, in the relay, after the
//! control-plane commit.
//!
//! The port speaks the domain chart of accounts ([`LedgerAccountKey`],
//! [`TransferCode`]); the adapter resolves keys to `u128` TigerBeetle ids (via the
//! `tb_accounts` map) and creates accounts with the correct non-negative flag on
//! first touch. Transfer `id`s are **caller-assigned and deterministic** (derived
//! from the event id), so a retried submit returns `Exists` — idempotent by design.
//! Amounts are always **explicit** (never TB balancing flags), so a retry moves the
//! exact amount frozen into the event.

use async_trait::async_trait;
use domain::{
	architecture::Gateway,
	balance::{LedgerAccountKey, TransferCode},
	error::DomainError,
	money::Usdt,
};
use thiserror::Error;

#[async_trait]
pub trait Ledger: Gateway {
	/// Ensure the account for `key` exists with the correct ledger, code, and
	/// non-negative flag. Idempotent; flags are set on first create and are
	/// immutable in TB thereafter, so the create must be right the first time.
	async fn ensure_account(&self, key: &LedgerAccountKey) -> Result<(), LedgerError>;

	/// Live balance for an account, normalized to its natural side (Read-First).
	async fn balance(&self, key: &LedgerAccountKey) -> Result<LedgerBalance, LedgerError>;

	/// Apply a posted transfer with an explicit amount. Ensures both accounts exist
	/// first. Idempotent on the transfer `id` (a re-submit returns `Exists` ⇒ ok).
	async fn post(&self, transfer: &LedgerTransfer) -> Result<(), LedgerError>;

	/// Apply a pending (two-phase) transfer with `timeout = 0` — the saga owns the
	/// lifecycle, never TB's clock (so a pending can't auto-void out from under it).
	async fn reserve(&self, transfer: &LedgerTransfer) -> Result<(), LedgerError>;

	/// Post or void a pending transfer. Already-posted/already-voided ⇒ success
	/// (idempotent); pending-not-found ⇒ [`LedgerError::Retryable`].
	async fn complete(&self, completion: &PendingCompletion) -> Result<(), LedgerError>;
}
/// Failure modes the relay and query handlers must distinguish — most importantly
/// `InsufficientFunds` (a real domain outcome from a non-negative-flag violation)
/// from `Retryable`/`Unavailable` (transient) and `Conflict` (a should-not-happen
/// the saga must surface, never silently absorb).
#[derive(Debug, Error)]
pub enum LedgerError {
	/// A non-negative invariant would be violated (TB `ExceedsCredits`/`ExceedsDebits`):
	/// the source account can't cover the transfer.
	#[error("insufficient funds")]
	InsufficientFunds,
	/// Transient — the ledger was unreachable or closed. Retry.
	#[error("ledger unavailable: {0}")]
	Unavailable(String),
	/// A two-phase post raced its pending create (`PendingTransferNotFound`). Retry
	/// after the pending lands.
	#[error("ledger retryable: {0}")]
	Retryable(String),
	/// A genuine inconsistency (overflow, expired pending, ledger mismatch). Park +
	/// alert; never treat as success.
	#[error("ledger conflict: {0}")]
	Conflict(String),
}

impl From<LedgerError> for DomainError {
	/// For a *query* read or a Read-First check: `InsufficientFunds` is a client-
	/// facing validation outcome; the rest are infrastructure faults (never leaked
	/// verbatim to clients — they map to `unavailable` at the gRPC boundary).
	fn from(err: LedgerError) -> Self {
		match err {
			LedgerError::InsufficientFunds => DomainError::Validation("insufficient funds".into()),
			LedgerError::Unavailable(detail) | LedgerError::Retryable(detail) | LedgerError::Conflict(detail) => DomainError::Repository(detail),
		}
	}
}

/// An account's live balance, normalized to its natural side (so both fields are
/// `>= 0` by the non-negative invariant). Zero when the account doesn't exist yet.
#[derive(Debug, Clone, Copy)]
pub struct LedgerBalance {
	pub posted: Usdt,
	pub pending: Usdt,
}

/// A posted or to-be-pending transfer. `id` is caller-assigned and deterministic;
/// `reference` is stamped into `user_data_128` (the allocation/deposit id) for
/// reconciliation.
#[derive(Debug, Clone)]
pub struct LedgerTransfer {
	pub id: u128,
	pub debit: LedgerAccountKey,
	pub credit: LedgerAccountKey,
	pub amount: Usdt,
	pub code: TransferCode,
	pub reference: u128,
}

/// Whether a [`PendingCompletion`] posts (commits) or voids (releases) the pending.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
	Post,
	Void,
}

/// Completes a previously-created pending transfer. Carries the original accounts
/// and amount so the adapter can re-issue the completion idempotently on retry.
#[derive(Debug, Clone)]
pub struct PendingCompletion {
	/// The completion transfer's own deterministic id.
	pub id: u128,
	/// The original pending transfer's id (TB `pending_id`).
	pub pending_id: u128,
	pub kind: CompletionKind,
	pub debit: LedgerAccountKey,
	pub credit: LedgerAccountKey,
	pub amount: Usdt,
	pub code: TransferCode,
	pub reference: u128,
}
