//! Postgres adapter for the [`UnitIssuanceRepository`] port.
//!
//! `issue` inserts the immutable issuance row — a mint or a retirement, told apart by
//! `source` — and drains its `Issued` event to the outbox in one
//! transaction. The `queued` → `applied` stamp and, for a user holder, the
//! `fund_positions` cost-basis projection are **not** written here: the relay applies
//! both after the leg posts (see [`super::relay::project_issuance`]), so a parked leg
//! can never leave a row claiming units that never moved, nor a phantom basis.
//!
//! Idempotency is the `(service, idempotency_key)` unique constraint, taken with
//! `ON CONFLICT DO NOTHING`: a lost race writes nothing — the event drain is gated on
//! the insert having landed — and the adapter hands back the row the winner wrote.
//!
//! A row naming the retired company stake (`holder_kind = 'company'` or
//! `source = 'company'`, both pre-#245 history) is read as
//! [`StoredIssuance::RetiredCompany`] here, at the boundary, and never reaches the
//! domain's parsers, which refuse those words.

use async_trait::async_trait;
use domain::{
	architecture::Repository,
	balance::ServiceId,
	error::DomainError,
	issuance::{IdempotencyKey, IssuanceSource, IssuanceState, UnitHolder, UnitIssuance, UnitIssuanceId, UnitIssuanceSnapshot},
	money::{Nav, Shares, Usdt},
	users::UserId,
};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::{
	infrastructure::{fee_accrual, outbox, rails::now_unix_i64},
	ports::issuance::{IssueOutcome, RetiredCompanyIssuance, RetiredCompanyMovement, StoredIssuance, UnitIssuanceRecord, UnitIssuanceRepository},
};

/// sqlx 0.9 accepts only `&'static str` SQL (its injection guardrail), so the shared
/// column list is spelled out per query rather than interpolated.
const SELECT_BY_KEY: &str = "SELECT id, service, holder_kind, holder_id, holder_service, source, units, nav, cost_basis, idempotency_key, state, \
	 EXTRACT(EPOCH FROM created_at)::bigint AS created_at, \
	 EXTRACT(EPOCH FROM applied_at)::bigint AS applied_at \
	 FROM unit_issuances WHERE service = $1 AND idempotency_key = $2";
const SELECT_BY_ID: &str = "SELECT id, service, holder_kind, holder_id, holder_service, source, units, nav, cost_basis, idempotency_key, state, \
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
	holder_service: Option<String>,
	source: String,
	units: String,
	nav: String,
	cost_basis: String,
	idempotency_key: String,
	state: String,
	created_at: i64,
	applied_at: Option<i64>,
}

/// The retired vocabulary, spelled once. The `unit_issuances` CHECKs refuse both on a new
/// row since 0047; the rows that hold them are history.
const RETIRED_COMPANY: &str = "company";

impl IssuanceRow {
	/// The row as a lookup answers it: live, or the retired company history.
	fn into_stored(self) -> Result<StoredIssuance, DomainError> {
		let movement = match (self.holder_kind.as_str(), self.source.as_str(), self.holder_id) {
			(RETIRED_COMPANY, "mint", None) => Some(RetiredCompanyMovement::MintedToCompany),
			("user", RETIRED_COMPANY, Some(user)) => Some(RetiredCompanyMovement::HandedTo(UserId::from_raw(user))),
			(RETIRED_COMPANY, _, _) | (_, RETIRED_COMPANY, _) => {
				return Err(DomainError::Repository(format!("issuance {} names the retired company stake in a shape it never had", self.id)));
			}
			_ => None,
		};
		let Some(movement) = movement else {
			return self.into_record().map(StoredIssuance::Live);
		};
		Ok(StoredIssuance::RetiredCompany(RetiredCompanyIssuance {
			id: UnitIssuanceId::from_raw(self.id),
			service: ServiceId::parse(&self.service)?,
			movement,
			units: Shares::from_base_units(parse_units(&self.units, "issuance units")?),
			created_at: self.created_at,
			applied_at: self.applied_at,
		}))
	}

	fn into_record(self) -> Result<UnitIssuanceRecord, DomainError> {
		let issuance = UnitIssuance::rehydrate(UnitIssuanceSnapshot {
			id: UnitIssuanceId::from_raw(self.id),
			service: ServiceId::parse(&self.service)?,
			holder: UnitHolder::from_parts(
				&self.holder_kind,
				self.holder_id.map(UserId::from_raw),
				self.holder_service.as_deref().map(ServiceId::parse).transpose()?,
			)?,
			source: IssuanceSource::parse(&self.source)?,
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

async fn find_by_key(conn: &mut PgConnection, service: &ServiceId, key: &IdempotencyKey) -> Result<Option<StoredIssuance>, DomainError> {
	sqlx::query_as::<_, IssuanceRow>(SELECT_BY_KEY)
		.bind(service.as_str())
		.bind(key.as_str())
		.fetch_optional(conn)
		.await
		.map_err(repo_err)?
		.map(IssuanceRow::into_stored)
		.transpose()
}

#[async_trait]
impl UnitIssuanceRepository for PgUnitIssuances {
	async fn issue(&self, issuance: &mut UnitIssuance) -> Result<IssueOutcome, DomainError> {
		let mut tx = self.pool.begin().await.map_err(repo_err)?;
		let inserted = sqlx::query(
			"INSERT INTO unit_issuances (id, service, holder_kind, holder_id, holder_service, source, units, nav, cost_basis, idempotency_key, state) \
			 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) \
			 ON CONFLICT (service, idempotency_key) DO NOTHING",
		)
		.bind(issuance.id().raw())
		.bind(issuance.service().as_str())
		.bind(issuance.holder().kind_str())
		.bind(issuance.holder().user_id().map(|user| user.raw()))
		.bind(issuance.holder().service_id().map(ServiceId::as_str))
		.bind(issuance.source().as_str())
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
			return match existing {
				StoredIssuance::Live(existing) => Ok(IssueOutcome::Existing(existing)),
				StoredIssuance::RetiredCompany(_) => Err(DomainError::Conflict(format!(
					"idempotency key '{}' already names a historical company-stake issuance on '{}'",
					issuance.idempotency_key().as_str(),
					issuance.service()
				))),
			};
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

	async fn find_by_key(&self, service: &ServiceId, key: &IdempotencyKey) -> Result<Option<StoredIssuance>, DomainError> {
		let mut conn = self.pool.acquire().await.map_err(repo_err)?;
		find_by_key(&mut conn, service, key).await
	}

	async fn find_by_id(&self, id: UnitIssuanceId) -> Result<Option<StoredIssuance>, DomainError> {
		sqlx::query_as::<_, IssuanceRow>(SELECT_BY_ID)
			.bind(id.raw())
			.fetch_optional(&self.pool)
			.await
			.map_err(repo_err)?
			.map(IssuanceRow::into_stored)
			.transpose()
	}

	async fn queued_mint_units(&self, service: &ServiceId) -> Result<Shares, DomainError> {
		// `units` is a base-unit digit string; summed as numeric so a u128 never has to
		// round-trip through a Postgres integer, and cast back to text for the parser.
		// Mints only, deliberately: a queued `retire` shrinks the supply, and a cap pinned
		// above where it lands is harmless — retires are allowed on closed products, where
		// nothing subscribes up to the cap anyway. Netting it would also push the figure
		// below zero when a retire waits with no mint beside it.
		let total: String = sqlx::query_scalar(
			"SELECT COALESCE(SUM(units::numeric), 0)::text FROM unit_issuances \
			 WHERE service = $1 AND state = 'queued' AND source = 'mint'",
		)
		.bind(service.as_str())
		.fetch_one(&self.pool)
		.await
		.map_err(repo_err)?;
		Ok(Shares::from_base_units(parse_units(&total, "queued mint units")?))
	}
}

/// What a holder's `fund_positions` projection gains from an applied issuance, inside the
/// caller's transaction. A user holder's position gains the units and the cost basis, with
/// the issuance's `nav` blended into the high-water mark exactly as a subscription at that
/// NAV would be — an investor handed units in kind is measured for performance fees from
/// the price they were handed them at — after the management accrual on the old basis has
/// been settled ([`fee_accrual::carry_accrual`], the obligation every writer of
/// `cost_basis` carries). An allocation holder gets no projection: there is no investor to
/// report P&L or charge fees to. A **retirement** runs the seller's side of a trade
/// instead — units off, basis down pro rata (clamped at zero), high-water mark untouched,
/// because nothing was realised at any price.
///
/// One writer: the relay, once the leg it posted has landed (`project_issuance`).
pub(crate) async fn project_holder_position(
	tx: &mut PgConnection,
	holder: &UnitHolder,
	source: IssuanceSource,
	service: &ServiceId,
	units: Shares,
	nav: Nav,
	cost_basis: Usdt,
) -> Result<(), sqlx::Error> {
	let UnitHolder::User(user) = holder else {
		return Ok(());
	};
	fee_accrual::carry_accrual(tx, user.raw(), service.as_str(), now_unix_i64())
		.await
		.map_err(|err| sqlx::Error::Protocol(format!("carry fee accrual before issuance basis change: {err}")))?;
	match source {
		IssuanceSource::Mint => {
			sqlx::query(
				"INSERT INTO fund_positions (user_id, service, cost_basis, units, high_water_mark) VALUES ($1, $2, $3, $4, $5) \
				 ON CONFLICT (user_id, service) DO UPDATE SET \
				 cost_basis = (fund_positions.cost_basis::numeric + EXCLUDED.cost_basis::numeric)::text, \
				 units = (fund_positions.units::numeric + EXCLUDED.units::numeric)::text, \
				 high_water_mark = GREATEST(fund_positions.high_water_mark::numeric, EXCLUDED.high_water_mark::numeric)::text, \
				 updated_at = now()",
			)
			.bind(user.raw())
			.bind(service.as_str())
			.bind(cost_basis.base_units().to_string())
			.bind(units.base_units().to_string())
			.bind(nav.base_units().to_string())
			.execute(&mut *tx)
			.await?;
		}
		// The row's own `cost_basis` is the book value the operator wrote off for the
		// record; the projection sheds its basis pro rata to the units retired, exactly as
		// a seller's does, so a holder who retires half keeps half of what they paid.
		IssuanceSource::Retire => {
			sqlx::query(
				"UPDATE fund_positions SET \
				 cost_basis = CASE WHEN units::numeric > $3::numeric THEN trunc(cost_basis::numeric * (units::numeric - $3::numeric) / units::numeric)::text ELSE '0' END, \
				 units = GREATEST(units::numeric - $3::numeric, 0)::text, \
				 updated_at = now() \
				 WHERE user_id = $1 AND service = $2",
			)
			.bind(user.raw())
			.bind(service.as_str())
			.bind(units.base_units().to_string())
			.execute(&mut *tx)
			.await?;
		}
	}
	Ok(())
}
