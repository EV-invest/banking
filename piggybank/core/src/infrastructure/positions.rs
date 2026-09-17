//! Postgres adapter for the [`FundPositionReader`] port — reads the `fund_positions`
//! projection (cost basis + high-water mark) maintained by subscribe/redeem, and the
//! holder's unit flows merged from the records of every channel that moves units.

use async_trait::async_trait;
use domain::{
	balance::ServiceId,
	error::DomainError,
	money::{Nav, Usdt},
	users::UserId,
};
use sqlx::{PgPool, Row};

use crate::ports::positions::{FundPosition, FundPositionReader, UnitFlow};

/// A holder's unit moves in one fund, merged in the database like the operation feed
/// is. Each branch is the control-plane record the write side already keeps, stamped at
/// the moment the units actually moved: a subscription at request (its mint is the very
/// next relay tick), a redemption at settlement (`updated_at` once `completed` — the
/// reserve at request leaves the units owned), a fee at assessment, an issuance when the
/// relay applied it, a book fill at execution — the buyer gains `size`, the seller loses
/// it. `delta` is signed TEXT: the tables store unsigned base units as TEXT (never a
/// number SQL reasons about), and the sign is the one thing added here. Seconds are
/// floored like the marks' (`nav.rs`), so a flow and a mark in the same second agree.
const UNIT_FLOWS_SQL: &str = "SELECT FLOOR(EXTRACT(EPOCH FROM ts))::bigint AS at_unix, delta FROM (
    SELECT created_at AS ts, units AS delta
      FROM subscriptions WHERE user_id = $1 AND service = $2
    UNION ALL
    SELECT updated_at, '-' || units
      FROM redemptions WHERE user_id = $1 AND service = $2 AND state = 'completed'
    UNION ALL
    SELECT assessed_at, '-' || charged_units
      FROM fee_assessments WHERE user_id = $1 AND service = $2 AND charged_units <> '0'
    UNION ALL
    SELECT applied_at, units
      FROM unit_issuances WHERE holder_kind = 'user' AND holder_id = $1 AND service = $2 AND state = 'applied'
    UNION ALL
    SELECT executed_at, size
      FROM book_trades WHERE buyer_id = $1 AND service = $2
    UNION ALL
    SELECT executed_at, '-' || size
      FROM book_trades WHERE seller_id = $1 AND service = $2
) AS flows
ORDER BY ts ASC";

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

	async fn unit_flows(&self, user: UserId, service: &ServiceId) -> Result<Vec<UnitFlow>, DomainError> {
		let rows = sqlx::query_as::<_, (i64, String)>(UNIT_FLOWS_SQL)
			.bind(user.raw())
			.bind(service.as_str())
			.fetch_all(&self.pool)
			.await
			.map_err(repo_err)?;
		rows.into_iter()
			.map(|(at_unix, delta)| {
				let delta = delta.parse::<i128>().map_err(|_| DomainError::Repository("malformed base-unit amount in a unit flow".into()))?;
				Ok(UnitFlow { at_unix, delta })
			})
			.collect()
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
