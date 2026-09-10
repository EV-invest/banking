//! Postgres adapter for the [`OutflowPolicy`] port — the control-plane facts every
//! payout path clears before money leaves.
//!
//! It borrows the pool rather than owning a clone: the two call sites (the dispatcher
//! sweep and the admin `DispatchWithdrawal` handler) both already hold one, and a
//! borrow-holder keeps the adapter out of [`AppState`](crate::AppState) — the policy is
//! a property of the withdrawal use case, not another shared handle to thread through
//! twenty constructor arguments.

use async_trait::async_trait;
use domain::{error::DomainError, users::UserId};
use sqlx::PgPool;

use crate::{
	infrastructure::operations,
	ports::outflow::{OutflowPolicy, PayoutStanding},
};

pub struct PgOutflowPolicy<'a> {
	pool: &'a PgPool,
}

impl<'a> PgOutflowPolicy<'a> {
	pub fn new(pool: &'a PgPool) -> Self {
		Self { pool }
	}
}

#[async_trait]
impl OutflowPolicy for PgOutflowPolicy<'_> {
	async fn outflows_paused(&self) -> Result<bool, DomainError> {
		operations::is_read_only(self.pool).await.map_err(|err| DomainError::Repository(err.to_string()))
	}

	async fn standing(&self, user: UserId) -> Result<Option<PayoutStanding>, DomainError> {
		let row: Option<(bool, i32)> = sqlx::query_as("SELECT (frozen OR status = 'disabled'), kyc_level FROM users WHERE id = $1")
			.bind(user.raw())
			.fetch_optional(self.pool)
			.await
			.map_err(|err| DomainError::Repository(err.to_string()))?;
		Ok(row.map(|(blocked, kyc_level)| PayoutStanding {
			blocked,
			// The column is a signed `int4` the bridge mirrors from the identity plane. A
			// negative (or otherwise impossible) tier is corrupt data, not a verification —
			// degrade it to 0 so the caller's floor refuses, the same way `role_of` degrades
			// an unparseable role to the one that holds nothing.
			kyc_level: u32::try_from(kyc_level).unwrap_or(0),
		}))
	}
}
