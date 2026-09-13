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
//!
//! Grants are rows of `allocation_access_grants`, written and removed under the
//! allocation's row lock so the audit fact the aggregate raises and the row it describes
//! land in one transaction. The caller's effective level is never stored: the read
//! queries `LEFT JOIN` the caller's grant and the domain folds the two columns
//! ([`AllocationAccess::effective`]), so the ranking lives in exactly one place. The
//! only SQL that reasons about levels is the catalog filter, and it needs no ranking —
//! a grant can carry nothing below `view`, so "has a grant" already means "may view".

use async_trait::async_trait;
use domain::{
	allocations::{Allocation, AllocationAccess, AllocationIcon, AllocationId, AllocationSnapshot, AllocationState},
	architecture::{EmitsEvents, Reader, Repository},
	balance::ServiceId,
	error::DomainError,
	money::Shares,
	users::UserId,
};
use sqlx::{PgConnection, PgPool};
use tracing::warn;
use uuid::Uuid;

use crate::{
	infrastructure::outbox,
	ports::allocations::{AllocationAccessGrant, AllocationRecord, AllocationRegistry},
};

/// sqlx 0.9 accepts only `&'static str` SQL (its injection guardrail), so the shared
/// column list is spelled out per query rather than interpolated.
const SELECT_BY_SERVICE: &str = "SELECT id, service, title, summary, state, unit_cap, icon, access, \
	 EXTRACT(EPOCH FROM created_at)::bigint AS created_at, \
	 EXTRACT(EPOCH FROM updated_at)::bigint AS updated_at \
	 FROM allocations WHERE service = $1";
const SELECT_BY_SERVICE_FOR_UPDATE: &str = "SELECT id, service, title, summary, state, unit_cap, icon, access, \
	 EXTRACT(EPOCH FROM created_at)::bigint AS created_at, \
	 EXTRACT(EPOCH FROM updated_at)::bigint AS updated_at \
	 FROM allocations WHERE service = $1 FOR UPDATE";
/// `$2` is the caller: their grant, if any, rides along as `grant_level`.
const SELECT_BY_SERVICE_FOR_CALLER: &str = "SELECT a.id, a.service, a.title, a.summary, a.state, a.unit_cap, a.icon, a.access, \
	 g.level AS grant_level, \
	 EXTRACT(EPOCH FROM a.created_at)::bigint AS created_at, \
	 EXTRACT(EPOCH FROM a.updated_at)::bigint AS updated_at \
	 FROM allocations a \
	 LEFT JOIN allocation_access_grants g ON g.service = a.service AND g.user_id = $2 \
	 WHERE a.service = $1";
/// `$1` is the caller, `$2` is `include_unlisted`. The visibility filter needs no
/// ranking: a grant can only carry `view` or `invest`, so its mere presence is "may
/// view", and the product's own default does the rest.
const SELECT_CATALOG_FOR_CALLER: &str = "SELECT a.id, a.service, a.title, a.summary, a.state, a.unit_cap, a.icon, a.access, \
	 g.level AS grant_level, \
	 EXTRACT(EPOCH FROM a.created_at)::bigint AS created_at, \
	 EXTRACT(EPOCH FROM a.updated_at)::bigint AS updated_at \
	 FROM allocations a \
	 LEFT JOIN allocation_access_grants g ON g.service = a.service AND g.user_id = $1 \
	 WHERE $2 OR (a.state = 'open' AND (a.access <> 'hidden' OR g.level IS NOT NULL)) \
	 ORDER BY a.service";
const SELECT_GRANTS: &str = "SELECT service, user_id, level, granted_by, \
	 EXTRACT(EPOCH FROM granted_at)::bigint AS granted_at \
	 FROM allocation_access_grants WHERE service = $1 ORDER BY user_id";
const SELECT_GRANT: &str = "SELECT service, user_id, level, granted_by, \
	 EXTRACT(EPOCH FROM granted_at)::bigint AS granted_at \
	 FROM allocation_access_grants WHERE service = $1 AND user_id = $2";
/// The `WHERE` on the conflict arm makes a repeat grant at the same level touch no row,
/// which is how the caller knows there is no fact to log. `granted_at` moves on a real
/// change so the operator's list dates the level that stands, not the first contact.
const UPSERT_GRANT: &str = "INSERT INTO allocation_access_grants (service, user_id, level, granted_by) VALUES ($1, $2, $3, $4) \
	 ON CONFLICT (service, user_id) DO UPDATE SET level = EXCLUDED.level, granted_by = EXCLUDED.granted_by, granted_at = now() \
	 WHERE allocation_access_grants.level IS DISTINCT FROM EXCLUDED.level";

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
	unit_cap: String,
	icon: String,
	access: String,
	created_at: i64,
	updated_at: i64,
}

/// [`AllocationRow`] plus the caller's grant from the `LEFT JOIN` — `None` when they
/// hold none.
#[derive(sqlx::FromRow)]
struct AllocationForCallerRow {
	#[sqlx(flatten)]
	row: AllocationRow,
	grant_level: Option<String>,
}

#[derive(sqlx::FromRow)]
struct GrantRow {
	service: String,
	user_id: Uuid,
	level: String,
	granted_by: Uuid,
	granted_at: i64,
}

impl AllocationRow {
	fn into_domain(self) -> Result<Allocation, DomainError> {
		let unit_cap = Shares::from_base_units(
			self.unit_cap
				.parse::<u128>()
				.map_err(|_| DomainError::Repository("malformed base-unit amount in allocations.unit_cap".into()))?,
		);
		// Deliberately laxer than the gRPC boundary, which refuses an unknown icon
		// (`AllocationsSvc::parse_icon`). The asymmetry is the point: a *client* sending
		// a value this build cannot draw is a request that can still be rejected, while a
		// *row* holding one is already written — and this mapper is on both `find` (the
		// gate every subscribe and redeem passes through) and `list` (the whole catalog),
		// so refusing here would take the money plane down over a presentation column.
		// The way that row appears is ordinary: widen the vocabulary, deploy, an operator
		// picks the new value, roll the release back. The shipped client already falls
		// back the same way (`shared/ui/icons/products.tsx`); the hub matches it.
		let icon = AllocationIcon::parse(&self.icon).unwrap_or_else(|_| {
			warn!(service = %self.service, icon = %self.icon, "allocations: stored icon is outside this build's vocabulary — rendering the default");
			AllocationIcon::default()
		});
		// `access` gets no such leniency: it gates money, and a value this build cannot
		// rank cannot be shown to be below `invest`. Failing the read is the safe answer.
		Ok(Allocation::rehydrate(AllocationSnapshot {
			id: AllocationId::from_raw(self.id),
			service: ServiceId::parse(&self.service)?,
			title: self.title,
			summary: self.summary,
			state: AllocationState::parse(&self.state)?,
			unit_cap,
			icon,
			access: AllocationAccess::parse(&self.access)?,
		}))
	}
}

impl AllocationForCallerRow {
	fn into_record(self) -> Result<AllocationRecord, DomainError> {
		let (created_at, updated_at) = (self.row.created_at, self.row.updated_at);
		let grant = self.grant_level.as_deref().map(AllocationAccess::parse).transpose()?;
		let allocation = self.row.into_domain()?;
		Ok(AllocationRecord {
			caller_access: AllocationAccess::effective(allocation.access(), grant),
			allocation,
			created_at,
			updated_at,
		})
	}
}

impl GrantRow {
	fn into_domain(self) -> Result<AllocationAccessGrant, DomainError> {
		Ok(AllocationAccessGrant {
			service: ServiceId::parse(&self.service)?,
			user_id: UserId::from_raw(self.user_id),
			level: AllocationAccess::parse(&self.level)?,
			granted_by: UserId::from_raw(self.granted_by),
			granted_at: self.granted_at,
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
	let result = sqlx::query("UPDATE allocations SET title = $2, summary = $3, state = $4, unit_cap = $5, icon = $6, access = $7, updated_at = now() WHERE id = $1")
		.bind(allocation.id().raw())
		.bind(allocation.title())
		.bind(allocation.summary())
		.bind(allocation.state().as_str())
		.bind(allocation.unit_cap().base_units().to_string())
		.bind(allocation.icon().as_str())
		.bind(allocation.access().as_str())
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
		let inserted =
			sqlx::query("INSERT INTO allocations (id, service, title, summary, state, unit_cap, icon, access) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) ON CONFLICT (service) DO NOTHING")
				.bind(allocation.id().raw())
				.bind(allocation.service().as_str())
				.bind(allocation.title())
				.bind(allocation.summary())
				.bind(allocation.state().as_str())
				.bind(allocation.unit_cap().base_units().to_string())
				.bind(allocation.icon().as_str())
				.bind(allocation.access().as_str())
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

	async fn update_details(&self, service: &ServiceId, title: &str, summary: &str, icon: Option<AllocationIcon>) -> Result<Allocation, DomainError> {
		self.transition(service, |allocation| allocation.update_details(title, summary, icon)).await
	}

	async fn set_unit_cap(&self, service: &ServiceId, unit_cap: Shares) -> Result<Allocation, DomainError> {
		self.transition(service, |allocation| allocation.set_unit_cap(unit_cap)).await
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

	async fn set_access(&self, service: &ServiceId, access: AllocationAccess) -> Result<Allocation, DomainError> {
		self.transition(service, |allocation| {
			allocation.set_access(access);
			Ok(())
		})
		.await
	}

	async fn grant_access(&self, service: &ServiceId, user: UserId, level: AllocationAccess, granted_by: UserId) -> Result<AllocationAccessGrant, DomainError> {
		let mut tx = self.pool.begin().await.map_err(repo_err)?;
		let mut allocation = load_for_update(&mut tx, service).await?;
		// Validated by the aggregate BEFORE the row is touched, so a level a grant may not
		// carry is refused as bad input rather than by the column CHECK as `internal`.
		allocation.grant_access(user, level, granted_by)?;
		let changed = sqlx::query(UPSERT_GRANT)
			.bind(service.as_str())
			.bind(user.raw())
			.bind(level.as_str())
			.bind(granted_by.raw())
			.execute(&mut *tx)
			.await
			.map_err(repo_err)?
			.rows_affected();
		if changed == 1 {
			outbox::drain_to_outbox(&mut tx, &mut allocation, false).await?;
		} else {
			// The grant already stood at this level: idempotent, and the fact the aggregate
			// raised describes nothing that happened, so it is dropped rather than logged.
			allocation.drain_events();
		}
		let row = sqlx::query_as::<_, GrantRow>(SELECT_GRANT)
			.bind(service.as_str())
			.bind(user.raw())
			.fetch_one(&mut *tx)
			.await
			.map_err(repo_err)?;
		tx.commit().await.map_err(repo_err)?;
		row.into_domain()
	}

	async fn revoke_access(&self, service: &ServiceId, user: UserId, revoked_by: UserId) -> Result<(), DomainError> {
		let mut tx = self.pool.begin().await.map_err(repo_err)?;
		let mut allocation = load_for_update(&mut tx, service).await?;
		let removed = sqlx::query("DELETE FROM allocation_access_grants WHERE service = $1 AND user_id = $2")
			.bind(service.as_str())
			.bind(user.raw())
			.execute(&mut *tx)
			.await
			.map_err(repo_err)?
			.rows_affected();
		// Only a grant that actually stood leaves a fact behind — revoking nothing is the
		// documented idempotent no-op.
		if removed == 1 {
			allocation.revoke_access(user, revoked_by);
			outbox::drain_to_outbox(&mut tx, &mut allocation, false).await?;
		}
		tx.commit().await.map_err(repo_err)?;
		Ok(())
	}

	async fn list_grants(&self, service: &ServiceId) -> Result<Vec<AllocationAccessGrant>, DomainError> {
		if self.find(service).await?.is_none() {
			return Err(not_found(service));
		}
		let rows = sqlx::query_as::<_, GrantRow>(SELECT_GRANTS)
			.bind(service.as_str())
			.fetch_all(&self.pool)
			.await
			.map_err(repo_err)?;
		rows.into_iter().map(GrantRow::into_domain).collect()
	}

	async fn find(&self, service: &ServiceId) -> Result<Option<Allocation>, DomainError> {
		let row = sqlx::query_as::<_, AllocationRow>(SELECT_BY_SERVICE)
			.bind(service.as_str())
			.fetch_optional(&self.pool)
			.await
			.map_err(repo_err)?;
		row.map(AllocationRow::into_domain).transpose()
	}

	async fn find_for(&self, service: &ServiceId, caller: UserId) -> Result<Option<AllocationRecord>, DomainError> {
		let row = sqlx::query_as::<_, AllocationForCallerRow>(SELECT_BY_SERVICE_FOR_CALLER)
			.bind(service.as_str())
			.bind(caller.raw())
			.fetch_optional(&self.pool)
			.await
			.map_err(repo_err)?;
		row.map(AllocationForCallerRow::into_record).transpose()
	}

	async fn list_for(&self, caller: UserId, include_unlisted: bool) -> Result<Vec<AllocationRecord>, DomainError> {
		let rows = sqlx::query_as::<_, AllocationForCallerRow>(SELECT_CATALOG_FOR_CALLER)
			.bind(caller.raw())
			.bind(include_unlisted)
			.fetch_all(&self.pool)
			.await
			.map_err(repo_err)?;
		rows.into_iter().map(AllocationForCallerRow::into_record).collect()
	}
}
