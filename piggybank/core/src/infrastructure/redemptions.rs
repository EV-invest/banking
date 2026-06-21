//! Postgres adapter for the [`RedemptionRepository`] port.
//!
//! Mirrors [`PgWithdrawals`](super::withdrawals::PgWithdrawals): row-locked commands
//! that apply the aggregate transition and drain its events in one transaction. `open`
//! also takes a `FOR UPDATE` lock on the user's `fund_positions` row to serialize
//! concurrent requests; `settle` reduces that position's cost basis proportionally to
//! the redeemed fraction (average cost) for P&L.

use async_trait::async_trait;
use domain::{
	architecture::{Reader, Repository},
	balance::ServiceId,
	error::DomainError,
	money::{Nav, Shares, Usdt},
	redemptions::{Redemption, RedemptionId, RedemptionState},
	users::UserId,
};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::{infrastructure::outbox, ports::RedemptionRepository};

const SELECT_BY_ID: &str = "SELECT id, user_id, service, units, nav, cash, state FROM redemptions WHERE id = $1";
const SELECT_BY_ID_FOR_UPDATE: &str = "SELECT id, user_id, service, units, nav, cash, state FROM redemptions WHERE id = $1 FOR UPDATE";
const SELECT_BY_USER: &str = "SELECT id, user_id, service, units, nav, cash, state FROM redemptions WHERE user_id = $1 ORDER BY created_at DESC";

pub struct PgRedemptions {
	pool: PgPool,
}

impl PgRedemptions {
	pub fn new(pool: PgPool) -> Self {
		Self { pool }
	}
}

impl Repository for PgRedemptions {
	type Aggregate = Redemption;
}

impl Reader for PgRedemptions {
	type Aggregate = Redemption;
}

#[derive(sqlx::FromRow)]
struct RedemptionRow {
	id: Uuid,
	user_id: Uuid,
	service: String,
	units: String,
	nav: Option<String>,
	cash: Option<String>,
	state: String,
}

impl RedemptionRow {
	fn into_domain(self) -> Result<Redemption, DomainError> {
		let nav = self.nav.as_deref().map(|s| parse_units(s, "redemption nav")).transpose()?.map(Nav::from_base_units);
		let cash = self.cash.as_deref().map(|s| parse_units(s, "redemption cash")).transpose()?.map(Usdt::from_base_units);
		Ok(Redemption::rehydrate(
			RedemptionId::from_raw(self.id),
			UserId::from_raw(self.user_id),
			ServiceId::parse(&self.service)?,
			Shares::from_base_units(parse_units(&self.units, "redemption units")?),
			nav,
			cash,
			RedemptionState::parse(&self.state)?,
		))
	}
}

fn parse_units(raw: &str, what: &str) -> Result<u128, DomainError> {
	raw.parse::<u128>().map_err(|_| DomainError::Repository(format!("malformed {what}")))
}

fn repo_err(err: sqlx::Error) -> DomainError {
	DomainError::Repository(err.to_string())
}

async fn insert_row(conn: &mut PgConnection, redemption: &Redemption) -> Result<(), DomainError> {
	sqlx::query("INSERT INTO redemptions (id, user_id, service, units, nav, cash, state) VALUES ($1, $2, $3, $4, $5, $6, $7)")
		.bind(redemption.id().raw())
		.bind(redemption.user().raw())
		.bind(redemption.service().as_str())
		.bind(redemption.units().base_units().to_string())
		.bind(redemption.nav().map(|n| n.base_units().to_string()))
		.bind(redemption.cash().map(|c| c.base_units().to_string()))
		.bind(redemption.state().as_str())
		.execute(&mut *conn)
		.await
		.map_err(repo_err)?;
	Ok(())
}

/// Persist a state transition — `state`, and `nav`/`cash` set at settle. We hold the row
/// lock, so exactly one row must update.
async fn update_row(conn: &mut PgConnection, redemption: &Redemption) -> Result<(), DomainError> {
	let result = sqlx::query("UPDATE redemptions SET state = $2, nav = $3, cash = $4, updated_at = now() WHERE id = $1")
		.bind(redemption.id().raw())
		.bind(redemption.state().as_str())
		.bind(redemption.nav().map(|n| n.base_units().to_string()))
		.bind(redemption.cash().map(|c| c.base_units().to_string()))
		.execute(&mut *conn)
		.await
		.map_err(repo_err)?;
	if result.rows_affected() != 1 {
		return Err(DomainError::Repository("redemption row vanished under lock".into()));
	}
	Ok(())
}

/// Lock + load a redemption for a state transition.
async fn load_for_update(conn: &mut PgConnection, id: RedemptionId) -> Result<Redemption, DomainError> {
	let row = sqlx::query_as::<_, RedemptionRow>(SELECT_BY_ID_FOR_UPDATE)
		.bind(id.raw())
		.fetch_optional(&mut *conn)
		.await
		.map_err(repo_err)?;
	row.ok_or_else(|| DomainError::NotFound {
		entity: "redemption",
		id: id.to_string(),
	})?
	.into_domain()
}

/// Reduce a position's average-cost basis by the redeemed fraction
/// (`cost_basis × remaining / units_held`, truncated). A no-op if the position row is
/// absent (a holding minted outside subscribe, e.g. a test).
async fn reduce_cost_basis(conn: &mut PgConnection, redemption: &Redemption, units_held: Shares) -> Result<(), DomainError> {
	let held = units_held.base_units();
	if held == 0 {
		return Ok(());
	}
	let remaining = held.saturating_sub(redemption.units().base_units());
	sqlx::query(
		"UPDATE fund_positions SET cost_basis = trunc(cost_basis::numeric * $3::numeric / $4::numeric)::text, updated_at = now() \
		 WHERE user_id = $1 AND service = $2",
	)
	.bind(redemption.user().raw())
	.bind(redemption.service().as_str())
	.bind(remaining.to_string())
	.bind(held.to_string())
	.execute(&mut *conn)
	.await
	.map_err(repo_err)?;
	Ok(())
}

#[async_trait]
impl RedemptionRepository for PgRedemptions {
	async fn open(&self, redemption: &mut Redemption) -> Result<(), DomainError> {
		let mut tx = self.pool.begin().await.map_err(repo_err)?;
		// Serialize concurrent requests on this position (TB is the over-redeem backstop).
		sqlx::query("SELECT 1 FROM fund_positions WHERE user_id = $1 AND service = $2 FOR UPDATE")
			.bind(redemption.user().raw())
			.bind(redemption.service().as_str())
			.fetch_optional(&mut *tx)
			.await
			.map_err(repo_err)?;
		insert_row(&mut tx, redemption).await?;
		outbox::drain_to_outbox(&mut tx, redemption, true).await?;
		tx.commit().await.map_err(repo_err)?;
		Ok(())
	}

	async fn settle(&self, id: RedemptionId, nav: Nav, units_held: Shares) -> Result<Redemption, DomainError> {
		let mut tx = self.pool.begin().await.map_err(repo_err)?;
		let mut redemption = load_for_update(&mut tx, id).await?;
		redemption.settle(nav)?;
		update_row(&mut tx, &redemption).await?;
		reduce_cost_basis(&mut tx, &redemption, units_held).await?;
		outbox::drain_to_outbox(&mut tx, &mut redemption, true).await?;
		tx.commit().await.map_err(repo_err)?;
		Ok(redemption)
	}

	async fn fail(&self, id: RedemptionId) -> Result<Redemption, DomainError> {
		let mut tx = self.pool.begin().await.map_err(repo_err)?;
		let mut redemption = load_for_update(&mut tx, id).await?;
		redemption.fail()?;
		update_row(&mut tx, &redemption).await?;
		outbox::drain_to_outbox(&mut tx, &mut redemption, true).await?;
		tx.commit().await.map_err(repo_err)?;
		Ok(redemption)
	}

	async fn cancel(&self, id: RedemptionId) -> Result<Redemption, DomainError> {
		let mut tx = self.pool.begin().await.map_err(repo_err)?;
		let mut redemption = load_for_update(&mut tx, id).await?;
		redemption.cancel()?;
		update_row(&mut tx, &redemption).await?;
		outbox::drain_to_outbox(&mut tx, &mut redemption, true).await?;
		tx.commit().await.map_err(repo_err)?;
		Ok(redemption)
	}

	async fn find_by_id(&self, id: RedemptionId) -> Result<Option<Redemption>, DomainError> {
		let row = sqlx::query_as::<_, RedemptionRow>(SELECT_BY_ID)
			.bind(id.raw())
			.fetch_optional(&self.pool)
			.await
			.map_err(repo_err)?;
		row.map(RedemptionRow::into_domain).transpose()
	}

	async fn list_by_user(&self, user: UserId) -> Result<Vec<Redemption>, DomainError> {
		let rows = sqlx::query_as::<_, RedemptionRow>(SELECT_BY_USER)
			.bind(user.raw())
			.fetch_all(&self.pool)
			.await
			.map_err(repo_err)?;
		rows.into_iter().map(RedemptionRow::into_domain).collect()
	}
}
