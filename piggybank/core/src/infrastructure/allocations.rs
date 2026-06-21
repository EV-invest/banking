//! Postgres adapter for the [`AllocationRepository`] port.
//!
//! The command methods are atomic and **row-locked**: load `FOR UPDATE`, apply the
//! aggregate command **inside the lock** (the aggregate is the single authority on a
//! transition's validity — the revoke rule lives there), then persist the new state
//! together with the drained events (event_log + outbox) in one transaction. A
//! transition emits an event only when it actually happens, so an idempotent
//! re-revoke writes no outbox row and the relay moves money exactly once.

use async_trait::async_trait;
use domain::{
	allocations::{Allocation, AllocationId, AllocationKind, AllocationState},
	architecture::{Reader, Repository},
	balance::Party,
	error::DomainError,
	money::Usdt,
	users::UserId,
};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::{infrastructure::outbox, ports::AllocationRepository};

const SELECT_BY_ID: &str = "SELECT id, amount, owner_kind, owner_id, sharers::text AS sharers, kind::text AS kind, state FROM allocations WHERE id = $1";
const SELECT_BY_ID_FOR_UPDATE: &str = "SELECT id, amount, owner_kind, owner_id, sharers::text AS sharers, kind::text AS kind, state FROM allocations WHERE id = $1 FOR UPDATE";
const SELECT_BY_USER: &str = "SELECT id, amount, owner_kind, owner_id, sharers::text AS sharers, kind::text AS kind, state FROM allocations WHERE user_id = $1 ORDER BY created_at DESC";
pub struct PgAllocations {
	pool: PgPool,
}

impl PgAllocations {
	pub fn new(pool: PgPool) -> Self {
		Self { pool }
	}
}

impl Repository for PgAllocations {
	type Aggregate = Allocation;
}

impl Reader for PgAllocations {
	type Aggregate = Allocation;
}

// Static query strings (sqlx 0.9 rejects non-`'static` SQL to force an injection
// audit; the only varying part here is the literal column list, so inline it).

#[derive(sqlx::FromRow)]
struct AllocationRow {
	id: Uuid,
	amount: String,
	owner_kind: String,
	owner_id: Option<String>,
	sharers: String,
	kind: String,
	state: String,
}

impl AllocationRow {
	fn into_domain(self) -> Result<Allocation, DomainError> {
		let amount = Usdt::from_base_units(self.amount.parse::<u128>().map_err(|_| DomainError::Repository("malformed allocation amount".into()))?);
		let sharers: Vec<Party> = serde_json::from_str(&self.sharers).map_err(|e| DomainError::Repository(e.to_string()))?;
		let kind: AllocationKind = serde_json::from_str(&self.kind).map_err(|e| DomainError::Repository(e.to_string()))?;
		Ok(Allocation::rehydrate(
			AllocationId::from_raw(self.id),
			amount,
			Party::from_parts(&self.owner_kind, self.owner_id.as_deref())?,
			sharers,
			kind,
			AllocationState::parse(&self.state)?,
		))
	}
}

fn repo_err(err: sqlx::Error) -> DomainError {
	DomainError::Repository(err.to_string())
}

/// The denormalized `(user_id, service_id)` query columns, derived from the kind.
fn denormalized(allocation: &Allocation) -> (Option<Uuid>, Option<String>) {
	match allocation.kind() {
		AllocationKind::UserStake { user, service } => (Some(user.raw()), Some(service.as_str().to_owned())),
		AllocationKind::ServiceReservation { service } | AllocationKind::ServiceHolding { service } => (None, Some(service.as_str().to_owned())),
	}
}

/// Persist the aggregate's full mutable state (so any transition — revoke, and later
/// settle/cancel which flip owner/kind — is covered by one writer).
async fn upsert_row(conn: &mut PgConnection, allocation: &Allocation, insert: bool) -> Result<(), DomainError> {
	let (user_id, service_id) = denormalized(allocation);
	let sharers = serde_json::to_string(allocation.sharers()).map_err(|e| DomainError::Repository(e.to_string()))?;
	let kind = serde_json::to_string(allocation.kind()).map_err(|e| DomainError::Repository(e.to_string()))?;
	let amount = allocation.amount().base_units().to_string();
	if insert {
		sqlx::query("INSERT INTO allocations (id, amount, owner_kind, owner_id, sharers, kind, user_id, service_id, state) VALUES ($1, $2, $3, $4, $5::jsonb, $6::jsonb, $7, $8, $9)")
			.bind(allocation.id().raw())
			.bind(amount)
			.bind(allocation.owner().kind_str())
			.bind(allocation.owner().id_str())
			.bind(sharers)
			.bind(kind)
			.bind(user_id)
			.bind(service_id)
			.bind(allocation.state().as_str())
			.execute(&mut *conn)
			.await
			.map_err(repo_err)?;
	} else {
		let result =
			sqlx::query("UPDATE allocations SET state = $2, owner_kind = $3, owner_id = $4, sharers = $5::jsonb, kind = $6::jsonb, service_id = $7, updated_at = now() WHERE id = $1")
				.bind(allocation.id().raw())
				.bind(allocation.state().as_str())
				.bind(allocation.owner().kind_str())
				.bind(allocation.owner().id_str())
				.bind(sharers)
				.bind(kind)
				.bind(service_id)
				.execute(&mut *conn)
				.await
				.map_err(repo_err)?;
		if result.rows_affected() != 1 {
			// We hold the row lock, so the row must exist and update exactly once.
			return Err(DomainError::Repository("allocation row vanished under lock".into()));
		}
	}
	Ok(())
}

#[async_trait]
impl AllocationRepository for PgAllocations {
	async fn open(&self, allocation: &mut Allocation) -> Result<(), DomainError> {
		let mut tx = self.pool.begin().await.map_err(repo_err)?;
		upsert_row(&mut tx, allocation, true).await?;
		outbox::drain_to_outbox(&mut tx, allocation, true).await?;
		tx.commit().await.map_err(repo_err)?;
		Ok(())
	}

	async fn revoke_user_stake(&self, id: AllocationId, user: UserId) -> Result<Allocation, DomainError> {
		let mut tx = self.pool.begin().await.map_err(repo_err)?;
		let row = sqlx::query_as::<_, AllocationRow>(SELECT_BY_ID_FOR_UPDATE)
			.bind(id.raw())
			.fetch_optional(&mut *tx)
			.await
			.map_err(repo_err)?;
		let mut allocation = row
			.ok_or_else(|| DomainError::NotFound {
				entity: "allocation",
				id: id.to_string(),
			})?
			.into_domain()?;

		// The stateful rule, applied under the lock. No-op (no event) if already
		// revoked; `Forbidden`/`Conflict` propagate. Only a real transition drains an
		// event, so the reversing transfer is issued at most once.
		allocation.revoke_by_user(user)?;

		upsert_row(&mut tx, &allocation, false).await?;
		outbox::drain_to_outbox(&mut tx, &mut allocation, true).await?;
		tx.commit().await.map_err(repo_err)?;
		Ok(allocation)
	}

	async fn find_by_id(&self, id: AllocationId) -> Result<Option<Allocation>, DomainError> {
		let row = sqlx::query_as::<_, AllocationRow>(SELECT_BY_ID)
			.bind(id.raw())
			.fetch_optional(&self.pool)
			.await
			.map_err(repo_err)?;
		row.map(AllocationRow::into_domain).transpose()
	}

	async fn list_by_user(&self, user: UserId) -> Result<Vec<Allocation>, DomainError> {
		let rows = sqlx::query_as::<_, AllocationRow>(SELECT_BY_USER)
			.bind(user.raw())
			.fetch_all(&self.pool)
			.await
			.map_err(repo_err)?;
		rows.into_iter().map(AllocationRow::into_domain).collect()
	}
}
