//! Postgres adapter for the [`PayoutGuard`] port — a thin wrapper over the existing
//! [`operations::is_read_only`] and [`bridge::is_frozen`] control-plane reads, so
//! `application::withdrawals::dispatch_withdrawal` can reach them without depending
//! on `sqlx` directly.

use async_trait::async_trait;
use domain::{error::DomainError, users::UserId};
use sqlx::PgPool;

use crate::{
	infrastructure::{bridge, operations},
	ports::PayoutGuard,
};

pub struct PgPayoutGuard {
	pool: PgPool,
}

impl PgPayoutGuard {
	pub fn new(pool: PgPool) -> Self {
		Self { pool }
	}
}

#[async_trait]
impl PayoutGuard for PgPayoutGuard {
	async fn is_read_only(&self) -> Result<bool, DomainError> {
		operations::is_read_only(&self.pool).await.map_err(|err| DomainError::Repository(err.to_string()))
	}

	async fn is_frozen(&self, user: UserId) -> Result<bool, DomainError> {
		bridge::is_frozen(&self.pool, user).await.map_err(|err| DomainError::Repository(err.to_string()))
	}
}
