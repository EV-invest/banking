//! The `native_spend` ledger — what the signer has committed to spending in native coin, per
//! `(wallet, network)`, over the policy's sliding window (migration 0005).
//!
//! The policy decides; this module only guarantees that the decision is made against a
//! consistent view and that two requests cannot both squeeze through an almost-spent window.
//! [`NativeSpendLedger::open`] starts a transaction, takes a per-`(wallet, network)` advisory
//! lock, sweeps rows older than the window and sums what is left; the caller consults the
//! policy with that sum and either [`SpendWindow::record`]s the new spend (commit) or drops
//! the window (rollback). The lock is transaction-scoped, so a dropped window releases it.
//!
//! Amounts cross the wire as decimal text: a `u128` wei figure fits neither `i64` nor any
//! sqlx-native numeric type, and `NUMERIC(39,0)` round-trips it exactly.

use std::time::Duration;

use domain::money::Network;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::error::SignerError;

/// The signer's native-spend ledger over its own database.
#[derive(Clone)]
pub struct NativeSpendLedger {
	pool: PgPool,
}

/// An open, locked view of one `(wallet, network)` window: the spend already recorded in it,
/// and the transaction the new spend goes into. Dropping it rolls back and records nothing.
pub struct SpendWindow<'a> {
	tx: Transaction<'a, Postgres>,
	wallet_id: Uuid,
	network: Network,
	spent: u128,
}

impl NativeSpendLedger {
	pub fn new(pool: PgPool) -> Self {
		Self { pool }
	}

	/// Lock `(wallet_id, network)`'s window, drop what has aged out of it and sum the rest.
	///
	/// The advisory lock keys on the two hashes rather than one combined hash so the SQL stays
	/// a plain two-argument call; a collision between unrelated pairs only serializes them,
	/// never lets two requests on the SAME pair run side by side.
	pub async fn open(&self, wallet_id: Uuid, network: Network, window: Duration) -> Result<SpendWindow<'_>, SignerError> {
		let mut tx = self.pool.begin().await?;
		sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1::text), hashtext($2))")
			.bind(wallet_id)
			.bind(network.as_str())
			.execute(&mut *tx)
			.await?;
		sqlx::query("DELETE FROM native_spend WHERE wallet_id = $1 AND network = $2 AND signed_at < now() - make_interval(secs => $3)")
			.bind(wallet_id)
			.bind(network.as_str())
			.bind(window.as_secs_f64())
			.execute(&mut *tx)
			.await?;
		let spent: String =
			sqlx::query_scalar("SELECT COALESCE(SUM(spend), 0)::text FROM native_spend WHERE wallet_id = $1 AND network = $2 AND signed_at >= now() - make_interval(secs => $3)")
				.bind(wallet_id)
				.bind(network.as_str())
				.bind(window.as_secs_f64())
				.fetch_one(&mut *tx)
				.await?;
		let spent = spent
			.parse::<u128>()
			.map_err(|_| SignerError::Repository(format!("native_spend window sum is not a u128: {spent:?}")))?;
		Ok(SpendWindow { tx, wallet_id, network, spent })
	}
}

impl SpendWindow<'_> {
	/// What the window already holds, before the spend being decided.
	pub fn spent(&self) -> u128 {
		self.spent
	}

	/// Record `spend` in the window and commit — the point of no return: from here the amount
	/// counts against the window whether or not the signature that follows succeeds.
	pub async fn record(mut self, spend: u128) -> Result<(), SignerError> {
		sqlx::query("INSERT INTO native_spend (wallet_id, network, spend) VALUES ($1, $2, $3::numeric)")
			.bind(self.wallet_id)
			.bind(self.network.as_str())
			.bind(spend.to_string())
			.execute(&mut *self.tx)
			.await?;
		self.tx.commit().await?;
		Ok(())
	}
}
