//! Postgres adapter for the [`SubscriptionRepository`] port.
//!
//! `open` writes the immutable subscription row, drains its `Subscribed` event to the
//! outbox, and upserts the per-investor `fund_positions` projection (cost basis +=
//! cash, high-water mark = max(hwm, nav)) — all in one transaction. Cost basis is the
//! average-cost net cash in, used for P&L; the high-water mark is reserved for a future
//! performance fee. The relay then posts the cash move and the unit mint.

use async_trait::async_trait;
use domain::{
	architecture::{Reader, Repository},
	error::DomainError,
	subscriptions::Subscription,
};
use sqlx::{PgConnection, PgPool};

use crate::{infrastructure::outbox, ports::SubscriptionRepository};

pub struct PgSubscriptions {
	pool: PgPool,
}

impl PgSubscriptions {
	pub fn new(pool: PgPool) -> Self {
		Self { pool }
	}
}

impl Repository for PgSubscriptions {
	type Aggregate = Subscription;
}

impl Reader for PgSubscriptions {
	type Aggregate = Subscription;
}

fn repo_err(err: sqlx::Error) -> DomainError {
	DomainError::Repository(err.to_string())
}

async fn insert_row(conn: &mut PgConnection, subscription: &Subscription) -> Result<(), DomainError> {
	sqlx::query("INSERT INTO subscriptions (id, user_id, service, cash, nav, units) VALUES ($1, $2, $3, $4, $5, $6)")
		.bind(subscription.id().raw())
		.bind(subscription.user().raw())
		.bind(subscription.service().as_str())
		.bind(subscription.cash().base_units().to_string())
		.bind(subscription.nav().base_units().to_string())
		.bind(subscription.units().base_units().to_string())
		.execute(&mut *conn)
		.await
		.map_err(repo_err)?;
	Ok(())
}

/// Upsert the per-investor position projection: cost basis accumulates the cash in
/// (average cost), the high-water mark tracks the highest NAV subscribed at. Integer
/// base-unit strings are summed via NUMERIC so arbitrary magnitudes stay exact.
async fn upsert_position(conn: &mut PgConnection, subscription: &Subscription) -> Result<(), DomainError> {
	sqlx::query(
		"INSERT INTO fund_positions (user_id, service, cost_basis, high_water_mark) VALUES ($1, $2, $3, $4) \
		 ON CONFLICT (user_id, service) DO UPDATE SET \
		 cost_basis = (fund_positions.cost_basis::numeric + EXCLUDED.cost_basis::numeric)::text, \
		 high_water_mark = GREATEST(fund_positions.high_water_mark::numeric, EXCLUDED.high_water_mark::numeric)::text, \
		 updated_at = now()",
	)
	.bind(subscription.user().raw())
	.bind(subscription.service().as_str())
	.bind(subscription.cash().base_units().to_string())
	.bind(subscription.nav().base_units().to_string())
	.execute(&mut *conn)
	.await
	.map_err(repo_err)?;
	Ok(())
}

#[async_trait]
impl SubscriptionRepository for PgSubscriptions {
	async fn open(&self, subscription: &mut Subscription) -> Result<(), DomainError> {
		let mut tx = self.pool.begin().await.map_err(repo_err)?;
		insert_row(&mut tx, subscription).await?;
		upsert_position(&mut tx, subscription).await?;
		outbox::drain_to_outbox(&mut tx, subscription, true).await?;
		tx.commit().await.map_err(repo_err)?;
		Ok(())
	}
}
