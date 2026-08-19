//! Postgres adapters for the fee ports.
//!
//! [`PgFeeAssessments::charge`] is the one that matters. It commits four things in a
//! single transaction, under the position's row lock:
//!
//! 1. the immutable `fee_assessments` audit row;
//! 2. the position's new `fee_debt`, `high_water_mark`, and accrual clocks;
//! 3. the projection's `units`, decremented by what was clawed back;
//! 4. the `Charged` event, into `event_log` **and** `outbox`.
//!
//! (3) is easy to miss and load-bearing. A redemption settle reduces the cost basis by
//! `(units − redeemed) / units` using the *projection's* unit count (migration 0010 —
//! the ledger's own balance lags the relay). If a clawback took units without telling
//! the projection, that denominator would be permanently too large and every later
//! settle would under-reduce the basis, overstating remaining capital and understating
//! realized gains. The same row lock the settle takes is what keeps the two from
//! interleaving.
//!
//! The clocks move only for what actually happened: `fees_accrued_at` on every charge
//! (management accrues continuously), `crystallized_at` only when the performance fee
//! actually crystallized — so a mid-period charge cannot silently restart the period.

use async_trait::async_trait;
use domain::{
	architecture::Repository,
	balance::ServiceId,
	error::DomainError,
	fees::{CrystallizationPeriod, FeeAssessment, FeePolicy, FeeSettlement, ManagementBasis, Trigger},
	money::{Nav, Shares, Usdt},
	users::UserId,
};
use sqlx::{PgConnection, PgPool, Row};
use uuid::Uuid;

use crate::{
	infrastructure::outbox,
	ports::fees::{AssessmentRecord, FeeAssessments, FeePolicies, FeeSettlements, PositionAccrual, PositionAccruals, SettlementRecord},
};

/// Cap on a returned history page. The fee statement grows by one row per position per
/// sweep, so an uncapped `SELECT` is unbounded in the length of the fund's life; nothing
/// downstream pages yet, and handing back a year of daily charges is already generous.
const HISTORY_LIMIT: i64 = 500;

pub(crate) fn repo_err(err: sqlx::Error) -> DomainError {
	DomainError::Repository(err.to_string())
}

pub(crate) fn parse_units(raw: &str, what: &str) -> Result<u128, DomainError> {
	raw.parse::<u128>().map_err(|_| DomainError::Repository(format!("malformed {what}")))
}

// ---------------------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------------------

pub struct PgFeePolicies {
	pool: PgPool,
}

impl PgFeePolicies {
	pub fn new(pool: PgPool) -> Self {
		Self { pool }
	}
}

pub(crate) fn policy_from_row(row: &sqlx::postgres::PgRow) -> Result<(ServiceId, FeePolicy), DomainError> {
	let service = ServiceId::parse(row.try_get::<String, _>("service").map_err(repo_err)?.as_str())?;
	let policy = FeePolicy::new(
		u32::try_from(row.try_get::<i32, _>("management_bps").map_err(repo_err)?).map_err(|_| DomainError::Repository("negative management rate".into()))?,
		u32::try_from(row.try_get::<i32, _>("performance_bps").map_err(repo_err)?).map_err(|_| DomainError::Repository("negative performance rate".into()))?,
		u32::try_from(row.try_get::<i32, _>("hurdle_bps").map_err(repo_err)?).map_err(|_| DomainError::Repository("negative hurdle rate".into()))?,
		ManagementBasis::parse(row.try_get::<String, _>("basis").map_err(repo_err)?.as_str())?,
		CrystallizationPeriod::parse(row.try_get::<String, _>("crystallization").map_err(repo_err)?.as_str())?,
	)?;
	Ok((service, policy))
}

#[async_trait]
impl FeePolicies for PgFeePolicies {
	async fn find(&self, service: &ServiceId) -> Result<Option<FeePolicy>, DomainError> {
		let row = sqlx::query("SELECT service, management_bps, performance_bps, hurdle_bps, basis, crystallization FROM fee_policies WHERE service = $1")
			.bind(service.as_str())
			.fetch_optional(&self.pool)
			.await
			.map_err(repo_err)?;
		row.map(|row| policy_from_row(&row).map(|(_, policy)| policy)).transpose()
	}

	async fn set(&self, service: &ServiceId, policy: FeePolicy, updated_by: &str) -> Result<(), DomainError> {
		sqlx::query(
			"INSERT INTO fee_policies (service, management_bps, performance_bps, hurdle_bps, basis, crystallization, updated_by) \
			 VALUES ($1, $2, $3, $4, $5, $6, $7) \
			 ON CONFLICT (service) DO UPDATE SET management_bps = EXCLUDED.management_bps, performance_bps = EXCLUDED.performance_bps, \
			   hurdle_bps = EXCLUDED.hurdle_bps, basis = EXCLUDED.basis, crystallization = EXCLUDED.crystallization, \
			   updated_by = EXCLUDED.updated_by, updated_at = now()",
		)
		.bind(service.as_str())
		.bind(i32::try_from(policy.management_bps()).unwrap_or(i32::MAX))
		.bind(i32::try_from(policy.performance_bps()).unwrap_or(i32::MAX))
		.bind(i32::try_from(policy.hurdle_bps()).unwrap_or(i32::MAX))
		.bind(policy.basis().as_str())
		.bind(policy.crystallization().as_str())
		.bind(updated_by)
		.execute(&self.pool)
		.await
		.map_err(|err| match &err {
			// The FK to `allocations` — terms for a product that was never registered.
			sqlx::Error::Database(db) if db.is_foreign_key_violation() => DomainError::NotFound {
				entity: "allocation",
				id: service.to_string(),
			},
			_ => repo_err(err),
		})?;
		Ok(())
	}

	async fn list(&self) -> Result<Vec<(ServiceId, FeePolicy)>, DomainError> {
		let rows = sqlx::query("SELECT service, management_bps, performance_bps, hurdle_bps, basis, crystallization FROM fee_policies ORDER BY service")
			.fetch_all(&self.pool)
			.await
			.map_err(repo_err)?;
		rows.iter().map(policy_from_row).collect()
	}
}

// ---------------------------------------------------------------------------------
// Accrual state (read side of `fund_positions`)
// ---------------------------------------------------------------------------------

pub struct PgPositionAccruals {
	pool: PgPool,
}

impl PgPositionAccruals {
	pub fn new(pool: PgPool) -> Self {
		Self { pool }
	}
}

fn accrual_from_row(row: &sqlx::postgres::PgRow) -> Result<PositionAccrual, DomainError> {
	Ok(PositionAccrual {
		user: UserId::from_raw(row.try_get::<Uuid, _>("user_id").map_err(repo_err)?),
		service: ServiceId::parse(row.try_get::<String, _>("service").map_err(repo_err)?.as_str())?,
		cost_basis: Usdt::from_base_units(parse_units(&row.try_get::<String, _>("cost_basis").map_err(repo_err)?, "cost basis")?),
		high_water_mark: Nav::from_base_units(parse_units(&row.try_get::<String, _>("high_water_mark").map_err(repo_err)?, "high-water mark")?),
		debt: Usdt::from_base_units(parse_units(&row.try_get::<String, _>("fee_debt").map_err(repo_err)?, "fee debt")?),
		accrued_at_unix: row.try_get("accrued_at_unix").map_err(repo_err)?,
		crystallized_at_unix: row.try_get("crystallized_at_unix").map_err(repo_err)?,
	})
}

#[async_trait]
impl PositionAccruals for PgPositionAccruals {
	async fn find(&self, user: UserId, service: &ServiceId) -> Result<Option<PositionAccrual>, DomainError> {
		let row = sqlx::query(
			"SELECT user_id, service, cost_basis, high_water_mark, fee_debt, \
			 EXTRACT(EPOCH FROM fees_accrued_at)::bigint AS accrued_at_unix, \
			 EXTRACT(EPOCH FROM crystallized_at)::bigint AS crystallized_at_unix \
			 FROM fund_positions WHERE user_id = $1 AND service = $2",
		)
		.bind(user.raw())
		.bind(service.as_str())
		.fetch_optional(&self.pool)
		.await
		.map_err(repo_err)?;
		row.as_ref().map(accrual_from_row).transpose()
	}

	async fn due(&self, accrued_before_unix: i64, limit: i64) -> Result<Vec<PositionAccrual>, DomainError> {
		let rows = sqlx::query(
			"SELECT user_id, service, cost_basis, high_water_mark, fee_debt, \
			 EXTRACT(EPOCH FROM fees_accrued_at)::bigint AS accrued_at_unix, \
			 EXTRACT(EPOCH FROM crystallized_at)::bigint AS crystallized_at_unix \
			 FROM fund_positions WHERE units <> '0' AND fees_accrued_at < to_timestamp($1) \
			 ORDER BY fees_accrued_at LIMIT $2",
		)
		.bind(accrued_before_unix)
		.bind(limit)
		.fetch_all(&self.pool)
		.await
		.map_err(repo_err)?;
		rows.iter().map(accrual_from_row).collect()
	}
}

// ---------------------------------------------------------------------------------
// The charge
// ---------------------------------------------------------------------------------

pub struct PgFeeAssessments {
	pool: PgPool,
}

impl PgFeeAssessments {
	pub fn new(pool: PgPool) -> Self {
		Self { pool }
	}
}

impl Repository for PgFeeAssessments {
	type Aggregate = FeeAssessment;
}

/// Lock the position row the charge is against. `NotFound` rather than a silent no-op:
/// a charge against a position that does not exist means the caller assessed a snapshot
/// that has since been deleted, and writing the audit row anyway would leave a charge
/// with nothing behind it.
async fn lock_position(conn: &mut PgConnection, user: UserId, service: &ServiceId) -> Result<Shares, DomainError> {
	let row = sqlx::query("SELECT units FROM fund_positions WHERE user_id = $1 AND service = $2 FOR UPDATE")
		.bind(user.raw())
		.bind(service.as_str())
		.fetch_optional(&mut *conn)
		.await
		.map_err(repo_err)?;
	let row = row.ok_or_else(|| DomainError::NotFound {
		entity: "fund position",
		id: format!("{user}/{service}"),
	})?;
	Ok(Shares::from_base_units(parse_units(&row.try_get::<String, _>("units").map_err(repo_err)?, "position units")?))
}

async fn insert_assessment(conn: &mut PgConnection, assessment: &FeeAssessment) -> Result<(), DomainError> {
	let charge = assessment.charge();
	sqlx::query(
		"INSERT INTO fee_assessments (id, user_id, service, trigger_kind, nav, management, performance, debt_opening, \
		   charged_units, charged_cash, debt_carried, high_water_mark) \
		 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
	)
	.bind(assessment.id().raw())
	.bind(assessment.user().raw())
	.bind(assessment.service().as_str())
	.bind(assessment.trigger().as_str())
	.bind(assessment.nav().base_units().to_string())
	.bind(charge.management.base_units().to_string())
	.bind(charge.performance.base_units().to_string())
	.bind(charge.debt_opening.base_units().to_string())
	.bind(charge.charged_units.base_units().to_string())
	.bind(charge.charged_cash.base_units().to_string())
	.bind(charge.debt_carried.base_units().to_string())
	.bind(charge.high_water_mark.base_units().to_string())
	.execute(&mut *conn)
	.await
	.map_err(repo_err)?;
	Ok(())
}

/// Advance the position: new debt, new mark, decremented units, and the clocks.
/// `crystallized_at` moves only when the performance fee actually crystallized.
///
/// Returns `false` when the position's accrual clock is no longer the one the caller
/// assessed against — someone else charged this holding between the snapshot read and
/// this write, and applying a second charge for the same elapsed window would bill the
/// investor twice. The row lock alone cannot see that: it serializes the two writes but
/// says nothing about the staleness of the second one's inputs. The clock *is* the
/// version, so comparing it is the whole check.
///
/// The comparison uses the same `EXTRACT(EPOCH …)::bigint` expression the snapshot was
/// read through, because rows predating this guard carry sub-second precision (the
/// migration backfilled them from `created_at`) and would never match a bare
/// `to_timestamp`.
///
/// The clocks are stamped from the caller's `now_unix` rather than Postgres's `now()`:
/// the charge was computed for the window ending at that instant, and stamping a later
/// one would silently drop the interval between the assessment and the commit.
async fn advance_position(conn: &mut PgConnection, assessment: &FeeAssessment, units_before: Shares, expected_accrued_at_unix: i64, now_unix: i64) -> Result<bool, DomainError> {
	let charge = assessment.charge();
	// Saturating: the projection is the authority for this subtraction, and a charge
	// capped by a *ledger* read could in principle exceed a lagging projection. Going
	// below zero is not representable and would abort the whole charge; clamping keeps
	// the projection monotone and the next sweep re-reads the truth from TigerBeetle.
	let units_after = units_before.checked_sub(charge.charged_units).unwrap_or(Shares::ZERO);
	let result = sqlx::query(
		"UPDATE fund_positions SET units = $3, fee_debt = $4, high_water_mark = $5, fees_accrued_at = to_timestamp($7), \
		   crystallized_at = CASE WHEN $6 THEN to_timestamp($7) ELSE crystallized_at END, updated_at = now() \
		 WHERE user_id = $1 AND service = $2 AND EXTRACT(EPOCH FROM fees_accrued_at)::bigint = $8",
	)
	.bind(assessment.user().raw())
	.bind(assessment.service().as_str())
	.bind(units_after.base_units().to_string())
	.bind(charge.debt_carried.base_units().to_string())
	.bind(charge.high_water_mark.base_units().to_string())
	.bind(charge.crystallized)
	.bind(now_unix)
	.bind(expected_accrued_at_unix)
	.execute(&mut *conn)
	.await
	.map_err(repo_err)?;
	Ok(result.rows_affected() == 1)
}

fn assessment_from_row(row: &sqlx::postgres::PgRow) -> Result<AssessmentRecord, DomainError> {
	let cash = |column: &str| -> Result<Usdt, DomainError> { Ok(Usdt::from_base_units(parse_units(&row.try_get::<String, _>(column).map_err(repo_err)?, column)?)) };
	Ok(AssessmentRecord {
		user: UserId::from_raw(row.try_get::<Uuid, _>("user_id").map_err(repo_err)?),
		service: ServiceId::parse(row.try_get::<String, _>("service").map_err(repo_err)?.as_str())?,
		trigger: match row.try_get::<String, _>("trigger_kind").map_err(repo_err)?.as_str() {
			"period" => Trigger::Period,
			// The units are recorded on the charge itself; a stored trigger only needs to
			// say which kind it was, so the redemption arm carries the charged units back.
			"redemption" => Trigger::Redemption {
				units: Shares::from_base_units(parse_units(&row.try_get::<String, _>("charged_units").map_err(repo_err)?, "charged units")?),
			},
			other => return Err(DomainError::Repository(format!("unknown fee trigger: {other}"))),
		},
		nav: Nav::from_base_units(parse_units(&row.try_get::<String, _>("nav").map_err(repo_err)?, "nav")?),
		management: cash("management")?,
		performance: cash("performance")?,
		debt_opening: cash("debt_opening")?,
		charged_units: Shares::from_base_units(parse_units(&row.try_get::<String, _>("charged_units").map_err(repo_err)?, "charged units")?),
		charged_cash: cash("charged_cash")?,
		debt_carried: cash("debt_carried")?,
		high_water_mark: Nav::from_base_units(parse_units(&row.try_get::<String, _>("high_water_mark").map_err(repo_err)?, "high-water mark")?),
		assessed_at_unix: row.try_get("assessed_at_unix").map_err(repo_err)?,
	})
}

#[async_trait]
impl FeeAssessments for PgFeeAssessments {
	async fn charge(&self, assessment: &mut FeeAssessment, expected_accrued_at_unix: i64, now_unix: i64) -> Result<bool, DomainError> {
		let mut tx = self.pool.begin().await.map_err(repo_err)?;
		let units_before = lock_position(&mut tx, assessment.user(), assessment.service()).await?;
		insert_assessment(&mut tx, assessment).await?;
		if !advance_position(&mut tx, assessment, units_before, expected_accrued_at_unix, now_unix).await? {
			// Someone charged this holding for the same window first. Roll the audit row
			// back with everything else and report the no-op — the events are still
			// undrained, so the aggregate leaves no trace either.
			tx.rollback().await.map_err(repo_err)?;
			return Ok(false);
		}
		outbox::drain_to_outbox(&mut tx, assessment, true).await?;
		tx.commit().await.map_err(repo_err)?;
		Ok(true)
	}

	async fn list_by_user(&self, user: UserId) -> Result<Vec<AssessmentRecord>, DomainError> {
		let rows = sqlx::query(
			"SELECT user_id, service, trigger_kind, nav, management, performance, debt_opening, \
			 charged_units, charged_cash, debt_carried, high_water_mark, EXTRACT(EPOCH FROM assessed_at)::bigint AS assessed_at_unix \
			 FROM fee_assessments WHERE user_id = $1 ORDER BY assessed_at DESC LIMIT $2",
		)
		.bind(user.raw())
		.bind(HISTORY_LIMIT)
		.fetch_all(&self.pool)
		.await
		.map_err(repo_err)?;
		rows.iter().map(assessment_from_row).collect()
	}

	async fn list_by_service(&self, service: &ServiceId) -> Result<Vec<AssessmentRecord>, DomainError> {
		let rows = sqlx::query(
			"SELECT user_id, service, trigger_kind, nav, management, performance, debt_opening, \
			 charged_units, charged_cash, debt_carried, high_water_mark, EXTRACT(EPOCH FROM assessed_at)::bigint AS assessed_at_unix \
			 FROM fee_assessments WHERE service = $1 ORDER BY assessed_at DESC LIMIT $2",
		)
		.bind(service.as_str())
		.bind(HISTORY_LIMIT)
		.fetch_all(&self.pool)
		.await
		.map_err(repo_err)?;
		rows.iter().map(assessment_from_row).collect()
	}
}

// ---------------------------------------------------------------------------------
// Bulk settlement of accumulated fee units
// ---------------------------------------------------------------------------------

pub struct PgFeeSettlements {
	pool: PgPool,
}

impl PgFeeSettlements {
	pub fn new(pool: PgPool) -> Self {
		Self { pool }
	}
}

impl Repository for PgFeeSettlements {
	type Aggregate = FeeSettlement;
}

#[async_trait]
impl FeeSettlements for PgFeeSettlements {
	async fn settle(&self, settlement: &mut FeeSettlement, settled_by: &str) -> Result<(), DomainError> {
		let mut tx = self.pool.begin().await.map_err(repo_err)?;
		sqlx::query("INSERT INTO fee_settlements (id, service, units, nav, cash, settled_by) VALUES ($1, $2, $3, $4, $5, $6)")
			.bind(settlement.id().raw())
			.bind(settlement.service().as_str())
			.bind(settlement.units().base_units().to_string())
			.bind(settlement.nav().base_units().to_string())
			.bind(settlement.cash().base_units().to_string())
			.bind(settled_by)
			.execute(&mut *tx)
			.await
			.map_err(repo_err)?;
		outbox::drain_to_outbox(&mut tx, settlement, true).await?;
		tx.commit().await.map_err(repo_err)?;
		Ok(())
	}

	async fn list_by_service(&self, service: &ServiceId) -> Result<Vec<SettlementRecord>, DomainError> {
		let rows = sqlx::query(
			"SELECT service, units, nav, cash, settled_by, EXTRACT(EPOCH FROM settled_at)::bigint AS settled_at_unix \
			 FROM fee_settlements WHERE service = $1 ORDER BY settled_at DESC LIMIT $2",
		)
		.bind(service.as_str())
		.bind(HISTORY_LIMIT)
		.fetch_all(&self.pool)
		.await
		.map_err(repo_err)?;
		rows.iter()
			.map(|row| {
				Ok(SettlementRecord {
					service: ServiceId::parse(row.try_get::<String, _>("service").map_err(repo_err)?.as_str())?,
					units: Shares::from_base_units(parse_units(&row.try_get::<String, _>("units").map_err(repo_err)?, "settlement units")?),
					nav: Nav::from_base_units(parse_units(&row.try_get::<String, _>("nav").map_err(repo_err)?, "settlement nav")?),
					cash: Usdt::from_base_units(parse_units(&row.try_get::<String, _>("cash").map_err(repo_err)?, "settlement cash")?),
					settled_by: row.try_get("settled_by").map_err(repo_err)?,
					settled_at_unix: row.try_get("settled_at_unix").map_err(repo_err)?,
				})
			})
			.collect()
	}
}
