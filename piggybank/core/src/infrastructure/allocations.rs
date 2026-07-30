//! Postgres adapter for the [`AllocationRegistry`] port.
//!
//! Mirrors [`PgRedemptions`](super::redemptions::PgRedemptions): row-locked commands
//! that apply the aggregate transition and drain its events in one transaction. The
//! drain passes `relay = false` — an allocation moves no value, so its events are audit
//! facts in `event_log` and must never reach the relay (which would park an event kind
//! it has no ledger op for).
//!
//! The unique `service` key is the concurrency control on registration: two racing
//! `RegisterAllocation` calls both insert, one hits the constraint, and it becomes a
//! `Conflict` rather than a second row for the same product.

use async_trait::async_trait;
use domain::{
	allocations::{Allocation, AllocationId, AllocationState},
	architecture::{Reader, Repository},
	balance::ServiceId,
	error::DomainError,
};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::{
	infrastructure::outbox,
	ports::allocations::{AllocationRecord, AllocationRegistry},
};

/// sqlx 0.9 accepts only `&'static str` SQL (its injection guardrail), so the shared
/// column list is spelled out per query rather than interpolated.
const SELECT_BY_SERVICE: &str = "SELECT id, service, title, summary, state, \
	 EXTRACT(EPOCH FROM created_at)::bigint AS created_at, \
	 EXTRACT(EPOCH FROM updated_at)::bigint AS updated_at \
	 FROM allocations WHERE service = $1";
const SELECT_BY_SERVICE_FOR_UPDATE: &str = "SELECT id, service, title, summary, state, \
	 EXTRACT(EPOCH FROM created_at)::bigint AS created_at, \
	 EXTRACT(EPOCH FROM updated_at)::bigint AS updated_at \
	 FROM allocations WHERE service = $1 FOR UPDATE";
/// `$1` is `include_unlisted`: false narrows to the investor-facing open catalog.
const SELECT_CATALOG: &str = "SELECT id, service, title, summary, state, \
	 EXTRACT(EPOCH FROM created_at)::bigint AS created_at, \
	 EXTRACT(EPOCH FROM updated_at)::bigint AS updated_at \
	 FROM allocations WHERE $1 OR state = 'open' ORDER BY service";

pub struct PgAllocations {
	pool: PgPool,
}

impl PgAllocations {
	pub fn new(pool: PgPool) -> Self {
		Self { pool }
	}

	/// Load, apply `command`, persist + drain — the shared shape of every transition.
	/// The row lock is held across the whole closure, so the aggregate never decides
	/// against a stale state.
	async fn transition<F>(&self, service: &ServiceId, command: F) -> Result<Allocation, DomainError>
	where
		F: FnOnce(&mut Allocation) -> Result<(), DomainError> + Send, {
		let mut tx = self.pool.begin().await.map_err(repo_err)?;
		let mut allocation = load_for_update(&mut tx, service).await?;
		command(&mut allocation)?;
		update_row(&mut tx, &allocation).await?;
		outbox::drain_to_outbox(&mut tx, &mut allocation, false).await?;
		tx.commit().await.map_err(repo_err)?;
		Ok(allocation)
	}
}

impl Repository for PgAllocations {
	type Aggregate = Allocation;
}

impl Reader for PgAllocations {
	type Aggregate = Allocation;
}

#[derive(sqlx::FromRow)]
struct AllocationRow {
	id: Uuid,
	service: String,
	title: String,
	summary: String,
	state: String,
	created_at: i64,
	updated_at: i64,
}

impl AllocationRow {
	fn into_domain(self) -> Result<Allocation, DomainError> {
		Ok(Allocation::rehydrate(
			AllocationId::from_raw(self.id),
			ServiceId::parse(&self.service)?,
			self.title,
			self.summary,
			AllocationState::parse(&self.state)?,
		))
	}

	fn into_record(self) -> Result<AllocationRecord, DomainError> {
		let (created_at, updated_at) = (self.created_at, self.updated_at);
		Ok(AllocationRecord {
			allocation: self.into_domain()?,
			created_at,
			updated_at,
		})
	}
}

fn repo_err(err: sqlx::Error) -> DomainError {
	DomainError::Repository(err.to_string())
}

fn not_found(service: &ServiceId) -> DomainError {
	DomainError::NotFound {
		entity: "allocation",
		id: service.to_string(),
	}
}

/// Lock + load an allocation for a transition.
async fn load_for_update(conn: &mut PgConnection, service: &ServiceId) -> Result<Allocation, DomainError> {
	let row = sqlx::query_as::<_, AllocationRow>(SELECT_BY_SERVICE_FOR_UPDATE)
		.bind(service.as_str())
		.fetch_optional(&mut *conn)
		.await
		.map_err(repo_err)?;
	row.ok_or_else(|| not_found(service))?.into_domain()
}

/// Persist the mutable fields. We hold the row lock, so exactly one row must update.
/// `service` and `id` are immutable and deliberately absent from the SET list.
async fn update_row(conn: &mut PgConnection, allocation: &Allocation) -> Result<(), DomainError> {
	let result = sqlx::query("UPDATE allocations SET title = $2, summary = $3, state = $4, updated_at = now() WHERE id = $1")
		.bind(allocation.id().raw())
		.bind(allocation.title())
		.bind(allocation.summary())
		.bind(allocation.state().as_str())
		.execute(&mut *conn)
		.await
		.map_err(repo_err)?;
	if result.rows_affected() != 1 {
		return Err(DomainError::Repository("allocation row vanished under lock".into()));
	}
	Ok(())
}

#[async_trait]
impl AllocationRegistry for PgAllocations {
	async fn register(&self, allocation: &mut Allocation) -> Result<(), DomainError> {
		let mut tx = self.pool.begin().await.map_err(repo_err)?;
		let inserted = sqlx::query("INSERT INTO allocations (id, service, title, summary, state) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (service) DO NOTHING")
			.bind(allocation.id().raw())
			.bind(allocation.service().as_str())
			.bind(allocation.title())
			.bind(allocation.summary())
			.bind(allocation.state().as_str())
			.execute(&mut *tx)
			.await
			.map_err(repo_err)?
			.rows_affected();
		if inserted != 1 {
			return Err(DomainError::Conflict(format!("allocation '{}' is already registered", allocation.service())));
		}
		outbox::drain_to_outbox(&mut tx, allocation, false).await?;
		tx.commit().await.map_err(repo_err)?;
		Ok(())
	}

	async fn update_details(&self, service: &ServiceId, title: &str, summary: &str) -> Result<Allocation, DomainError> {
		self.transition(service, |allocation| allocation.update_details(title, summary)).await
	}

	async fn open(&self, service: &ServiceId) -> Result<Allocation, DomainError> {
		self.transition(service, |allocation| {
			allocation.open();
			Ok(())
		})
		.await
	}

	async fn close(&self, service: &ServiceId) -> Result<Allocation, DomainError> {
		self.transition(service, |allocation| {
			allocation.close();
			Ok(())
		})
		.await
	}

	async fn find(&self, service: &ServiceId) -> Result<Option<Allocation>, DomainError> {
		let row = sqlx::query_as::<_, AllocationRow>(SELECT_BY_SERVICE)
			.bind(service.as_str())
			.fetch_optional(&self.pool)
			.await
			.map_err(repo_err)?;
		row.map(AllocationRow::into_domain).transpose()
	}

	async fn list(&self, include_unlisted: bool) -> Result<Vec<AllocationRecord>, DomainError> {
		let rows = sqlx::query_as::<_, AllocationRow>(SELECT_CATALOG)
			.bind(include_unlisted)
			.fetch_all(&self.pool)
			.await
			.map_err(repo_err)?;
		rows.into_iter().map(AllocationRow::into_record).collect()
	}
}
