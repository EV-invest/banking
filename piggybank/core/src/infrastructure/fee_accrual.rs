//! Settling the management accrual before the cost basis moves.
//!
//! The management leg is `basis × rate × elapsed`, and both factors live on the same
//! `fund_positions` row. Every writer of `cost_basis` therefore has an obligation the
//! fee plane cannot discharge on its own: a top-up raises the basis without touching the
//! elapsed clock, so an assessment running afterwards charges the *entire* elapsed window
//! on money that only arrived at the end of it.
//!
//! Bounded by the sweeper's daily cadence that error is small. It is not bounded at all
//! for a position that went to zero units and came back: [`super::fees`]'s work queue
//! skips unit-less rows (`units <> '0'`), so a dormant position's clock freezes for the
//! whole dormancy, and an investor who exits and returns a year later is billed a year of
//! management fee on their new capital.
//!
//! Resetting the clock alone would fix that and open a worse hole — an investor could top
//! up a dollar a day and never be charged at all, because the elapsed window would never
//! reach the sweeper's minimum age. So the accrual is **settled, not discarded**: what the
//! old basis earned is computed here and carried into `fee_debt`, exactly as an
//! uncollectable charge is carried, and the next assessment collects it as `debt_opening`.

use domain::{
	error::DomainError,
	fees::{self, FeePolicy, ManagementBasis, PositionSnapshot},
	money::{Nav, Shares, Usdt},
};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

use crate::infrastructure::fees::{parse_units, policy_from_row, repo_err};

/// The unit price standing in for "no price is needed" — the invested-capital basis is a
/// cash figure, and [`fees::management_due`] only reads the price on the market-value arm.
const UNUSED_PRICE: u128 = 1;

/// Carry the management fee accrued so far into `fee_debt` and restart the elapsed clock.
///
/// Runs inside the caller's transaction and re-takes the position row lock the caller
/// already holds — a no-op there, and it keeps the function correct if it is ever called
/// on its own. A row that does not exist yet needs nothing: the insert stamps both clocks
/// `now()`.
pub async fn carry_accrual(conn: &mut PgConnection, user: Uuid, service: &str, now_unix: i64) -> Result<(), DomainError> {
	let Some(row) = sqlx::query(
		"SELECT cost_basis, units, high_water_mark, fee_debt, \
		 EXTRACT(EPOCH FROM fees_accrued_at)::bigint AS accrued_at_unix, \
		 EXTRACT(EPOCH FROM crystallized_at)::bigint AS crystallized_at_unix \
		 FROM fund_positions WHERE user_id = $1 AND service = $2 FOR UPDATE",
	)
	.bind(user)
	.bind(service)
	.fetch_optional(&mut *conn)
	.await
	.map_err(repo_err)?
	else {
		return Ok(());
	};

	let units = Shares::from_base_units(parse_units(&row.try_get::<String, _>("units").map_err(repo_err)?, "position units")?);
	// A dormant position holds nothing, so nothing accrued while it slept — and its
	// performance clock must restart too, or the returning investor's first period would
	// be measured from an entry price they have already exited at.
	if units.is_zero() {
		sqlx::query("UPDATE fund_positions SET fees_accrued_at = to_timestamp($3), crystallized_at = to_timestamp($3) WHERE user_id = $1 AND service = $2")
			.bind(user)
			.bind(service)
			.bind(now_unix)
			.execute(&mut *conn)
			.await
			.map_err(repo_err)?;
		return Ok(());
	}

	let accrued = accrued_management(&mut *conn, &row, service, units, now_unix).await?;
	let debt = Usdt::from_base_units(parse_units(&row.try_get::<String, _>("fee_debt").map_err(repo_err)?, "fee debt")?);
	let carried = debt.checked_add(accrued).ok_or_else(|| DomainError::Repository("carried fee debt overflows".into()))?;

	sqlx::query("UPDATE fund_positions SET fee_debt = $3, fees_accrued_at = to_timestamp($4) WHERE user_id = $1 AND service = $2")
		.bind(user)
		.bind(service)
		.bind(carried.base_units().to_string())
		.bind(now_unix)
		.execute(&mut *conn)
		.await
		.map_err(repo_err)?;
	Ok(())
}

/// What the *current* basis has earned since the last accrual. Zero when the product has
/// no policy or charges no management fee.
///
/// The `market_value` basis needs a price, and this is the one place it is read without
/// the dealing staleness guard: a stale mark must not stop a subscription from settling.
/// With no mark at all the invested-capital figure stands in — it undercharges rather than
/// guessing a price, which is the direction every rounding decision in this plane leans.
async fn accrued_management(conn: &mut PgConnection, row: &sqlx::postgres::PgRow, service: &str, units: Shares, now_unix: i64) -> Result<Usdt, DomainError> {
	let Some(stored) = sqlx::query("SELECT service, management_bps, performance_bps, hurdle_bps, basis, crystallization FROM fee_policies WHERE service = $1")
		.bind(service)
		.fetch_optional(&mut *conn)
		.await
		.map_err(repo_err)?
	else {
		return Ok(Usdt::ZERO);
	};
	let (_, policy) = policy_from_row(&stored)?;
	if policy.management_bps() == 0 {
		return Ok(Usdt::ZERO);
	}

	let snapshot = PositionSnapshot {
		units,
		cost_basis: Usdt::from_base_units(parse_units(&row.try_get::<String, _>("cost_basis").map_err(repo_err)?, "cost basis")?),
		high_water_mark: Nav::from_base_units(parse_units(&row.try_get::<String, _>("high_water_mark").map_err(repo_err)?, "high-water mark")?),
		debt: Usdt::ZERO,
		accrued_at_unix: row.try_get("accrued_at_unix").map_err(repo_err)?,
		crystallized_at_unix: row.try_get("crystallized_at_unix").map_err(repo_err)?,
	};

	match policy.basis() {
		ManagementBasis::InvestedCapital => fees::management_due(&policy, &snapshot, Nav::from_base_units(UNUSED_PRICE), now_unix),
		ManagementBasis::MarketValue => match latest_nav(&mut *conn, service).await? {
			Some(nav) => fees::management_due(&policy, &snapshot, nav, now_unix),
			None => {
				let fallback = FeePolicy::new(policy.management_bps(), 0, 0, ManagementBasis::InvestedCapital, policy.crystallization())?;
				fees::management_due(&fallback, &snapshot, Nav::from_base_units(UNUSED_PRICE), now_unix)
			}
		},
	}
}

/// The most recent posted mark, staleness ignored. `None` when the fund has never been
/// valued.
async fn latest_nav(conn: &mut PgConnection, service: &str) -> Result<Option<Nav>, DomainError> {
	let row = sqlx::query("SELECT nav FROM fund_valuations WHERE service = $1 ORDER BY posted_at DESC LIMIT 1")
		.bind(service)
		.fetch_optional(&mut *conn)
		.await
		.map_err(repo_err)?;
	row.map(|row| Ok(Nav::from_base_units(parse_units(&row.try_get::<String, _>("nav").map_err(repo_err)?, "nav")?)))
		.transpose()
}
