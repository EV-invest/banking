//! TigerBeetle ledger gateway — the [`Ledger`] adapter.
//!
//! Translates the domain chart of accounts into TigerBeetle `Account`/`Transfer`
//! rows: resolves a [`LedgerAccountKey`] to a `u128` account id (minted once and
//! stored in the `tb_accounts` control-plane map), creates the account with the
//! correct **non-negative flag on first touch** (flags are immutable in TB), and
//! issues posted/pending transfers with **explicit amounts** and **caller-assigned
//! deterministic ids** (a re-submit returns `Exists` ⇒ idempotent). The `tb_accounts`
//! map is the adapter's own concern, read/written outside any command transaction —
//! which is exactly why [`Ledger`] is a `Gateway` (it can't join a `UnitOfWork`).

use std::sync::Arc;

use async_trait::async_trait;
use domain::{
	architecture::Gateway,
	balance::{LedgerAccountKey, Normal},
	money::Usdt,
};
use sqlx::PgPool;
use tigerbeetle as tb;
use uuid::Uuid;

use crate::{
	infrastructure::tigerbeetle::TigerBeetle,
	ports::ledger::{CompletionKind, Ledger, LedgerBalance, LedgerError, LedgerTransfer, PendingCompletion},
};

/// The TigerBeetle-backed [`Ledger`]. Holds the shared TB client and a Postgres pool
/// for the `tb_accounts` id-map only (never an amount).
pub struct TbLedger {
	tb: Arc<TigerBeetle>,
	pool: PgPool,
}

impl TbLedger {
	pub fn new(tb: Arc<TigerBeetle>, pool: PgPool) -> Self {
		Self { tb, pool }
	}

	/// Look up an existing account id from the map (no allocation).
	async fn resolve_id(&self, logical_key: &str) -> Result<Option<u128>, LedgerError> {
		let row = sqlx::query_scalar::<_, Vec<u8>>("SELECT tb_account_id FROM tb_accounts WHERE logical_key = $1")
			.bind(logical_key)
			.fetch_optional(&self.pool)
			.await
			.map_err(|e| LedgerError::Unavailable(format!("account map read: {e}")))?;
		row.map(|bytes| u128_from_be(&bytes)).transpose()
	}

	/// Resolve the account id for `key`, minting and recording a fresh `u128` on
	/// first use. Idempotent and race-safe (`ON CONFLICT DO NOTHING` + re-read),
	/// mirroring [`PgUsers::provision`](crate::infrastructure::users).
	async fn resolve_or_allocate(&self, key: &LedgerAccountKey) -> Result<u128, LedgerError> {
		let logical_key = key.logical_key();
		if let Some(id) = self.resolve_id(&logical_key).await? {
			return Ok(id);
		}
		let fresh = Uuid::new_v4().as_u128();
		let bytes = fresh.to_be_bytes();
		let inserted = sqlx::query_scalar::<_, Vec<u8>>(
			"INSERT INTO tb_accounts (logical_key, tb_account_id, ledger, code, network) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (logical_key) DO NOTHING RETURNING tb_account_id",
		)
		.bind(&logical_key)
		.bind(&bytes[..])
		.bind(key.ledger().id() as i32)
		.bind(key.account_code().code() as i32)
		.bind(key.network().map(|n| n.as_str()))
		.fetch_optional(&self.pool)
		.await
		.map_err(|e| LedgerError::Unavailable(format!("account map insert: {e}")))?;
		match inserted {
			Some(bytes) => u128_from_be(&bytes),
			// Lost the race — another writer inserted the same logical key first.
			None => self.resolve_id(&logical_key).await?.ok_or_else(|| LedgerError::Unavailable("account map race".into())),
		}
	}

	/// Resolve the id for `key` and ensure its TigerBeetle account exists (idempotent;
	/// always safe to call before a transfer, since the map insert and the TB create
	/// are not one atomic step).
	async fn ensure(&self, key: &LedgerAccountKey) -> Result<u128, LedgerError> {
		let id = self.resolve_or_allocate(key).await?;
		self.create_accounts(&[tb_account(id, key)]).await?;
		Ok(id)
	}

	/// Resolve + create both legs of a transfer in one account batch.
	async fn ensure_pair(&self, debit: &LedgerAccountKey, credit: &LedgerAccountKey) -> Result<(u128, u128), LedgerError> {
		let debit_id = self.resolve_or_allocate(debit).await?;
		let credit_id = self.resolve_or_allocate(credit).await?;
		self.create_accounts(&[tb_account(debit_id, debit), tb_account(credit_id, credit)]).await?;
		Ok((debit_id, credit_id))
	}

	async fn create_accounts(&self, accounts: &[tb::Account]) -> Result<(), LedgerError> {
		let results = self
			.tb
			.client()
			.create_accounts(accounts)
			.map_err(|e| LedgerError::Unavailable(format!("tigerbeetle closed: {e:?}")))?
			.await
			.map_err(|e| LedgerError::Unavailable(format!("create_accounts: {e:?}")))?;
		for result in results {
			match result.status {
				// `Created` (new) and `Exists` (same id+fields) are both success.
				tb::CreateAccountStatus::Created | tb::CreateAccountStatus::Exists => {}
				other => return Err(LedgerError::Conflict(format!("create account: {other:?}"))),
			}
		}
		Ok(())
	}

	async fn create_transfers(&self, transfers: &[tb::Transfer]) -> Result<(), LedgerError> {
		let results = self
			.tb
			.client()
			.create_transfers(transfers)
			.map_err(|e| LedgerError::Unavailable(format!("tigerbeetle closed: {e:?}")))?
			.await
			.map_err(|e| LedgerError::Unavailable(format!("create_transfers: {e:?}")))?;
		for result in results {
			map_transfer_status(result.status)?;
		}
		Ok(())
	}
}

impl Gateway for TbLedger {}

#[async_trait]
impl Ledger for TbLedger {
	async fn ensure_account(&self, key: &LedgerAccountKey) -> Result<(), LedgerError> {
		self.ensure(key).await.map(|_| ())
	}

	async fn balance(&self, key: &LedgerAccountKey) -> Result<LedgerBalance, LedgerError> {
		let Some(id) = self.resolve_id(&key.logical_key()).await? else {
			// No id-map row ⇒ the account was never touched ⇒ balance is zero.
			return Ok(LedgerBalance {
				posted: Usdt::ZERO,
				pending: Usdt::ZERO,
			});
		};
		let accounts = self
			.tb
			.client()
			.lookup_accounts(&[id])
			.map_err(|e| LedgerError::Unavailable(format!("tigerbeetle closed: {e:?}")))?
			.await
			.map_err(|e| LedgerError::Unavailable(format!("lookup_accounts: {e:?}")))?;
		let Some(account) = accounts.first() else {
			return Ok(LedgerBalance {
				posted: Usdt::ZERO,
				pending: Usdt::ZERO,
			});
		};
		// Normalize to the account's natural side; by the non-negative flag the
		// subtraction can't underflow, so a `None` here is a ledger inconsistency.
		let (posted, pending) = match key.normal() {
			Normal::Credit => (
				account.credits_posted.checked_sub(account.debits_posted),
				account.credits_pending.checked_sub(account.debits_pending),
			),
			Normal::Debit => (
				account.debits_posted.checked_sub(account.credits_posted),
				account.debits_pending.checked_sub(account.credits_pending),
			),
		};
		let posted = posted.ok_or_else(|| LedgerError::Conflict("balance underflow".into()))?;
		let pending = pending.ok_or_else(|| LedgerError::Conflict("balance underflow".into()))?;
		Ok(LedgerBalance {
			posted: Usdt::from_base_units(posted),
			pending: Usdt::from_base_units(pending),
		})
	}

	async fn post(&self, transfer: &LedgerTransfer) -> Result<(), LedgerError> {
		let (debit_id, credit_id) = self.ensure_pair(&transfer.debit, &transfer.credit).await?;
		let row = tb::Transfer {
			id: transfer.id,
			debit_account_id: debit_id,
			credit_account_id: credit_id,
			amount: transfer.amount.base_units(),
			ledger: transfer.debit.ledger().id(),
			code: transfer.code.code(),
			user_data_128: transfer.reference,
			..Default::default()
		};
		self.create_transfers(&[row]).await
	}

	async fn reserve(&self, transfer: &LedgerTransfer) -> Result<(), LedgerError> {
		let (debit_id, credit_id) = self.ensure_pair(&transfer.debit, &transfer.credit).await?;
		let row = tb::Transfer {
			id: transfer.id,
			debit_account_id: debit_id,
			credit_account_id: credit_id,
			amount: transfer.amount.base_units(),
			ledger: transfer.debit.ledger().id(),
			code: transfer.code.code(),
			user_data_128: transfer.reference,
			// `timeout = 0`: the saga owns the lifecycle, not TB's clock — a pending
			// can never auto-void out from under a settle/cancel.
			flags: tb::TransferFlags::Pending,
			..Default::default()
		};
		self.create_transfers(&[row]).await
	}

	async fn complete(&self, completion: &PendingCompletion) -> Result<(), LedgerError> {
		let (debit_id, credit_id) = self.ensure_pair(&completion.debit, &completion.credit).await?;
		let (flags, amount) = match completion.kind {
			// Post the full reserved amount; void ignores amount (TB requires 0).
			CompletionKind::Post => (tb::TransferFlags::PostPendingTransfer, completion.amount.base_units()),
			CompletionKind::Void => (tb::TransferFlags::VoidPendingTransfer, 0),
		};
		let row = tb::Transfer {
			id: completion.id,
			debit_account_id: debit_id,
			credit_account_id: credit_id,
			amount,
			pending_id: completion.pending_id,
			ledger: completion.debit.ledger().id(),
			code: completion.code.code(),
			user_data_128: completion.reference,
			flags,
			..Default::default()
		};
		self.create_transfers(&[row]).await
	}
}

/// Ensure the fund's singleton accounts exist at boot: a custody wallet and a fund-
/// capital claim per network, plus the mocked bank custody. Per-user/-service claim
/// accounts are created lazily on first transfer.
pub async fn seed_singletons(ledger: &dyn Ledger) -> Result<(), LedgerError> {
	use domain::money::Network;
	for network in Network::ALL {
		ledger.ensure_account(&LedgerAccountKey::CryptoWallet(network)).await?;
		ledger.ensure_account(&LedgerAccountKey::Fund(network)).await?;
	}
	ledger.ensure_account(&LedgerAccountKey::BankCustody).await?;
	Ok(())
}

/// Map a single TigerBeetle transfer status to a [`LedgerError`]. The idempotent-
/// success set is `Created | Exists | AlreadyPosted | AlreadyVoided`; a post racing
/// its pending create is `Retryable`; a non-negative violation is `InsufficientFunds`;
/// everything else is a genuine inconsistency to surface, never absorb.
fn map_transfer_status(status: tb::CreateTransferStatus) -> Result<(), LedgerError> {
	use tb::CreateTransferStatus as S;
	match status {
		S::Created | S::Exists | S::PendingTransferAlreadyPosted | S::PendingTransferAlreadyVoided => Ok(()),
		S::ExceedsCredits | S::ExceedsDebits => Err(LedgerError::InsufficientFunds),
		S::PendingTransferNotFound => Err(LedgerError::Retryable("pending transfer not found yet".into())),
		other => Err(LedgerError::Conflict(format!("create transfer: {other:?}"))),
	}
}

fn tb_account(id: u128, key: &LedgerAccountKey) -> tb::Account {
	tb::Account {
		id,
		ledger: key.ledger().id(),
		code: key.account_code().code(),
		flags: account_flags(key.normal()),
		..Default::default()
	}
}

/// The non-negative guard for an account's natural side — set once at create and
/// immutable thereafter, so a claim can never be over-spent nor custody go negative.
fn account_flags(normal: Normal) -> tb::AccountFlags {
	match normal {
		Normal::Debit => tb::AccountFlags::CreditsMustNotExceedDebits,
		Normal::Credit => tb::AccountFlags::DebitsMustNotExceedCredits,
	}
}

fn u128_from_be(bytes: &[u8]) -> Result<u128, LedgerError> {
	let array: [u8; 16] = bytes.try_into().map_err(|_| LedgerError::Conflict("malformed ledger account id".into()))?;
	Ok(u128::from_be_bytes(array))
}
