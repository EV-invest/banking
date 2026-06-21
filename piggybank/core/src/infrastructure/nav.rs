//! Postgres adapter for the [`NavRepository`] port — append-only fund valuation marks.
//!
//! Amounts/prices are stored as exact integer base-unit strings (the money-plane
//! convention) and parsed back to the typed `Usdt`/`Shares`/`Nav` on read. `posted_at`
//! is DB-stamped; `extract(epoch …)` exposes it as unix seconds for the staleness guard.

use async_trait::async_trait;
use domain::{
	balance::ServiceId,
	error::DomainError,
	money::{Nav, Shares, Usdt},
};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::ports::nav::{NavRepository, Valuation};

pub struct PgNav {
	pool: PgPool,
}

impl PgNav {
	pub fn new(pool: PgPool) -> Self {
		Self { pool }
	}
}

#[async_trait]
impl NavRepository for PgNav {
	async fn current(&self, service: &ServiceId) -> Result<Option<Valuation>, DomainError> {
		let row = sqlx::query(
			"SELECT aum, units_outstanding, nav, posted_by, EXTRACT(EPOCH FROM posted_at)::bigint AS posted_at_unix \
			 FROM fund_valuations WHERE service = $1 ORDER BY posted_at DESC LIMIT 1",
		)
		.bind(service.as_str())
		.fetch_optional(&self.pool)
		.await
		.map_err(repo_err)?;
		row.map(|row| valuation_from_row(service, &row)).transpose()
	}

	async fn record(&self, id: Uuid, service: &ServiceId, aum: Usdt, units_outstanding: Shares, nav: Nav, posted_by: &str) -> Result<i64, DomainError> {
		let posted_at_unix = sqlx::query_scalar::<_, i64>(
			"INSERT INTO fund_valuations (id, service, aum, units_outstanding, nav, posted_by) \
			 VALUES ($1, $2, $3, $4, $5, $6) RETURNING EXTRACT(EPOCH FROM posted_at)::bigint",
		)
		.bind(id)
		.bind(service.as_str())
		.bind(aum.base_units().to_string())
		.bind(units_outstanding.base_units().to_string())
		.bind(nav.base_units().to_string())
		.bind(posted_by)
		.fetch_one(&self.pool)
		.await
		.map_err(repo_err)?;
		Ok(posted_at_unix)
	}

	async fn history(&self, service: &ServiceId) -> Result<Vec<Valuation>, DomainError> {
		let rows = sqlx::query(
			"SELECT aum, units_outstanding, nav, posted_by, EXTRACT(EPOCH FROM posted_at)::bigint AS posted_at_unix \
			 FROM fund_valuations WHERE service = $1 ORDER BY posted_at DESC",
		)
		.bind(service.as_str())
		.fetch_all(&self.pool)
		.await
		.map_err(repo_err)?;
		rows.iter().map(|row| valuation_from_row(service, row)).collect()
	}
}

fn valuation_from_row(service: &ServiceId, row: &sqlx::postgres::PgRow) -> Result<Valuation, DomainError> {
	Ok(Valuation {
		service: service.clone(),
		aum: Usdt::from_base_units(parse_base_units(row.try_get("aum").map_err(repo_err)?)?),
		units_outstanding: Shares::from_base_units(parse_base_units(row.try_get("units_outstanding").map_err(repo_err)?)?),
		nav: Nav::from_base_units(parse_base_units(row.try_get("nav").map_err(repo_err)?)?),
		posted_by: row.try_get("posted_by").map_err(repo_err)?,
		posted_at_unix: row.try_get("posted_at_unix").map_err(repo_err)?,
	})
}

fn parse_base_units(raw: String) -> Result<u128, DomainError> {
	raw.parse::<u128>().map_err(|_| DomainError::Repository("malformed base-unit amount in fund_valuations".into()))
}

fn repo_err(err: sqlx::Error) -> DomainError {
	DomainError::Repository(err.to_string())
}
