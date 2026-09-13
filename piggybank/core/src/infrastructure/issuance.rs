//! Postgres adapter for the [`UnitIssuanceRepository`] port.
//!
//! `issue` inserts the immutable issuance row and drains its `Issued` event to the
//! outbox in one transaction. The `queued` → `applied` stamp and, for a user holder,
//! the `fund_positions` cost-basis projection are **not** written here: the relay
//! applies both after the mint posts (see [`super::relay::project_issuance`]), so a
//! parked mint can never leave a row claiming units that were never minted, nor a
//! phantom basis.
//!
//! Idempotency is the `(service, idempotency_key)` unique constraint, taken with
//! `ON CONFLICT DO NOTHING`: a lost race writes nothing — the event drain is gated on
//! the insert having landed — and the adapter hands back the row the winner wrote.

use async_trait::async_trait;
use domain::{
	architecture::Repository,
	balance::ServiceId,
	error::DomainError,
	issuance::{IdempotencyKey, IssuanceState, UnitHolder, UnitIssuance, UnitIssuanceId, UnitIssuanceSnapshot},
	money::{Nav, Shares, Usdt},
	users::UserId,
};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::{
	infrastructure::outbox,
	ports::issuance::{IssueOutcome, UnitIssuanceRecord, UnitIssuanceRepository},
};

/// sqlx 0.9 accepts only `&'static str` SQL (its injection guardrail), so the shared
/// column list is spelled out per query rather than interpolated.
const SELECT_BY_KEY: &str = "SELECT id, service, holder_kind, holder_id, units, nav, cost_basis, idempotency_key, state, \
	 EXTRACT(EPOCH FROM created_at)::bigint AS created_at, \
	 EXTRACT(EPOCH FROM applied_at)::bigint AS applied_at \
	 FROM unit_issuances WHERE service = $1 AND idempotency_key = $2";
const SELECT_BY_ID: &str = "SELECT id, service, holder_kind, holder_id, units, nav, cost_basis, idempotency_key, state, \
	 EXTRACT(EPOCH FROM created_at)::bigint AS created_at, \
	 EXTRACT(EPOCH FROM applied_at)::bigint AS applied_at \
	 FROM unit_issuances WHERE id = $1";

pub struct PgUnitIssuances {
	pool: PgPool,
}

impl PgUnitIssuances {
	pub fn new(pool: PgPool) -> Self {
		Self { pool }
	}
}

impl Repository for PgUnitIssuances {
	type Aggregate = UnitIssuance;
}

#[derive(sqlx::FromRow)]
struct IssuanceRow {
	id: Uuid,
	service: String,
	holder_kind: String,
	holder_id: Option<Uuid>,
	units: String,
	nav: String,
	cost_basis: String,
	idempotency_key: String,
	state: String,
	created_at: i64,
	applied_at: Option<i64>,
}

impl IssuanceRow {
	fn into_record(self) -> Result<UnitIssuanceRecord, DomainError> {
		let issuance = UnitIssuance::rehydrate(UnitIssuanceSnapshot {
			id: UnitIssuanceId::from_raw(self.id),
			service: ServiceId::parse(&self.service)?,
			holder: UnitHolder::from_parts(&self.holder_kind, self.holder_id.map(UserId::from_raw))?,
			units: Shares::from_base_units(parse_units(&self.units, "issuance units")?),
			nav: Nav::from_base_units(parse_units(&self.nav, "issuance nav")?),
			cost_basis: Usdt::from_base_units(parse_units(&self.cost_basis, "issuance cost basis")?),
			idempotency_key: IdempotencyKey::parse(&self.idempotency_key)?,
			state: IssuanceState::parse(&self.state)?,
		});
		Ok(UnitIssuanceRecord {
			issuance,
			created_at: self.created_at,
			applied_at: self.applied_at,
		})
	}
}

fn parse_units(raw: &str, what: &str) -> Result<u128, DomainError> {
	raw.parse::<u128>().map_err(|_| DomainError::Repository(format!("malformed {what}")))
}

fn repo_err(err: sqlx::Error) -> DomainError {
	DomainError::Repository(err.to_string())
}

async fn find_by_key(conn: &mut PgConnection, service: &ServiceId, key: &IdempotencyKey) -> Result<Option<UnitIssuanceRecord>, DomainError> {
	sqlx::query_as::<_, IssuanceRow>(SELECT_BY_KEY)
		.bind(service.as_str())
		.bind(key.as_str())
		.fetch_optional(conn)
		.await
		.map_err(repo_err)?
		.map(IssuanceRow::into_record)
		.transpose()
}

#[async_trait]
impl UnitIssuanceRepository for PgUnitIssuances {
	async fn issue(&self, issuance: &mut UnitIssuance) -> Result<IssueOutcome, DomainError> {
		let mut tx = self.pool.begin().await.map_err(repo_err)?;
		let inserted = sqlx::query(
			"INSERT INTO unit_issuances (id, service, holder_kind, holder_id, units, nav, cost_basis, idempotency_key, state) \
			 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
			 ON CONFLICT (service, idempotency_key) DO NOTHING",
		)
		.bind(issuance.id().raw())
		.bind(issuance.service().as_str())
		.bind(issuance.holder().kind_str())
		.bind(issuance.holder().user_id().map(|user| user.raw()))
		.bind(issuance.units().base_units().to_string())
		.bind(issuance.nav().base_units().to_string())
		.bind(issuance.cost_basis().base_units().to_string())
		.bind(issuance.idempotency_key().as_str())
		.bind(issuance.state().as_str())
		.execute(&mut *tx)
		.await
		.map_err(repo_err)?
		.rows_affected();
		if inserted == 0 {
			// A concurrent send of the same request won the key. Nothing of ours is
			// written — the aggregate keeps its undrained event, which the caller drops —
			// and the winner's row is the answer both callers get.
			let existing = find_by_key(&mut tx, issuance.service(), issuance.idempotency_key())
				.await?
				.ok_or_else(|| DomainError::Repository("issuance key conflicted but no row was found".into()))?;
			tx.commit().await.map_err(repo_err)?;
			return Ok(IssueOutcome::Existing(existing));
		}
		outbox::drain_to_outbox(&mut tx, issuance, true).await?;
		let recorded = sqlx::query_as::<_, IssuanceRow>(SELECT_BY_ID)
			.bind(issuance.id().raw())
			.fetch_one(&mut *tx)
			.await
			.map_err(repo_err)?
			.into_record()?;
		tx.commit().await.map_err(repo_err)?;
		Ok(IssueOutcome::Recorded(recorded))
	}

	async fn find_by_key(&self, service: &ServiceId, key: &IdempotencyKey) -> Result<Option<UnitIssuanceRecord>, DomainError> {
		let mut conn = self.pool.acquire().await.map_err(repo_err)?;
		find_by_key(&mut conn, service, key).await
	}

	async fn find_by_id(&self, id: UnitIssuanceId) -> Result<Option<UnitIssuanceRecord>, DomainError> {
		sqlx::query_as::<_, IssuanceRow>(SELECT_BY_ID)
			.bind(id.raw())
			.fetch_optional(&self.pool)
			.await
			.map_err(repo_err)?
			.map(IssuanceRow::into_record)
			.transpose()
	}
}
