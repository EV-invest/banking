//! Postgres adapter for the [`FundPositionReader`] port — reads the `fund_positions`
//! projection (cost basis + high-water mark) maintained by subscribe/redeem.

use async_trait::async_trait;
use domain::{
	balance::ServiceId,
	error::DomainError,
	money::{Nav, Usdt},
	users::UserId,
};
use sqlx::{PgPool, Row};

use crate::ports::positions::{FundPosition, FundPositionReader};

pub struct PgFundPositions {
	pool: PgPool,
}

impl PgFundPositions {
	pub fn new(pool: PgPool) -> Self {
		Self { pool }
	}
}

#[async_trait]
impl FundPositionReader for PgFundPositions {
	async fn find(&self, user: UserId, service: &ServiceId) -> Result<Option<FundPosition>, DomainError> {
		let row = sqlx::query("SELECT service, cost_basis, high_water_mark FROM fund_positions WHERE user_id = $1 AND service = $2")
			.bind(user.raw())
			.bind(service.as_str())
			.fetch_optional(&self.pool)
			.await
			.map_err(repo_err)?;
		row.map(|row| position_from_row(&row)).transpose()
	}

	async fn list(&self, user: UserId) -> Result<Vec<FundPosition>, DomainError> {
		let rows = sqlx::query("SELECT service, cost_basis, high_water_mark FROM fund_positions WHERE user_id = $1 ORDER BY service")
			.bind(user.raw())
			.fetch_all(&self.pool)
			.await
			.map_err(repo_err)?;
		rows.iter().map(position_from_row).collect()
	}
}

fn position_from_row(row: &sqlx::postgres::PgRow) -> Result<FundPosition, DomainError> {
	Ok(FundPosition {
		service: ServiceId::parse(row.try_get::<String, _>("service").map_err(repo_err)?.as_str())?,
		cost_basis: Usdt::from_base_units(parse_base_units(row.try_get("cost_basis").map_err(repo_err)?)?),
		high_water_mark: Nav::from_base_units(parse_base_units(row.try_get("high_water_mark").map_err(repo_err)?)?),
	})
}

fn parse_base_units(raw: String) -> Result<u128, DomainError> {
	raw.parse::<u128>().map_err(|_| DomainError::Repository("malformed base-unit amount in fund_positions".into()))
}

fn repo_err(err: sqlx::Error) -> DomainError {
	DomainError::Repository(err.to_string())
}
