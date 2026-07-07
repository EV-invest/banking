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

	/// The cash plane's global posted invariant, summed straight from TigerBeetle (the
	/// authoritative store): total custody (`wallet:<net>` debit-normal assets) vs total
	/// claims (`fund`/`user`/`service`/`fee`/`clearing` credit-normal). By construction
	/// `custody == claims` always holds; reconciliation asserts it and alerts if TB and
	/// the design ever diverge. Returns raw 18-dp USDT base units.
	async fn cash_invariant(&self) -> Result<CashInvariant, LedgerError>;
}
/// The reconciliation read of the cash plane's global double-entry invariant: the summed
/// posted custody side and claims side. They must be equal (`balanced()`).
#[derive(Clone, Copy, Debug)]
pub struct CashInvariant {
	pub custody: u128,
	pub claims: u128,
}

impl CashInvariant {
	pub fn balanced(self) -> bool {
		self.custody == self.claims
	}
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

/// An account's live balance in **raw base units**, normalized to its natural side
/// (every field `>= 0`). The unit is the account's ledger's (18-dp USDT for the cash
/// ledger, 18-dp shares for the Share ledger) — the gateway is currency-agnostic, so
/// callers wrap into the typed `Usdt`/`Shares` at the boundary. Zero when the account
/// doesn't exist yet.
#[derive(Clone, Copy, Debug)]
pub struct LedgerBalance {
	/// The settled balance on the natural side (`credits − debits` for a claim).
	pub posted: u128,
	/// In-flight INFLOW on the natural side (pending credits for a claim) awaiting
	/// settlement — zero for the common one-sided pending.
	pub pending: u128,
	/// In-flight OUTFLOW reserved against this account (pending debits on a claim):
	/// the amount locked by an unsettled withdrawal or reservation. Subtract from
	/// `posted` for the spendable balance ([`LedgerBalance::available`]).
	pub locked: u128,
}

impl LedgerBalance {
	/// The settled balance not already reserved by an in-flight pending — what a new
	/// command may actually spend (Read-First). Saturating, so never negative.
	pub fn available(self) -> u128 {
		self.posted.saturating_sub(self.locked)
	}
}

/// A posted or to-be-pending transfer. `id` is caller-assigned and deterministic;
/// `amount` is in the ledger's base units (`Usdt`/`Shares` converted via `base_units()`
/// by the relay); `reference` is stamped into `user_data_128` (the aggregate id) for
/// reconciliation.
#[derive(Clone, Debug)]
pub struct LedgerTransfer {
	pub id: u128,
	pub debit: LedgerAccountKey,
	pub credit: LedgerAccountKey,
	pub amount: u128,
	pub code: TransferCode,
	pub reference: u128,
}

/// Whether a [`PendingCompletion`] posts (commits) or voids (releases) the pending.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionKind {
	Post,
	Void,
}

/// Completes a previously-created pending transfer. Carries the original accounts
/// and amount so the adapter can re-issue the completion idempotently on retry.
#[derive(Clone, Debug)]
pub struct PendingCompletion {
	/// The completion transfer's own deterministic id.
	pub id: u128,
	/// The original pending transfer's id (TB `pending_id`).
	pub pending_id: u128,
	pub kind: CompletionKind,
	pub debit: LedgerAccountKey,
	pub credit: LedgerAccountKey,
	pub amount: u128,
	pub code: TransferCode,
	pub reference: u128,
}
