//! Postgres adapter for the [`FeePolicyChanges`] port — the history of a product's fee terms
//! and the ONLY writer of `fee_policies`.
//!
//! Two transactions here are load-bearing:
//!
//! - [`FeePolicyChanges::schedule`] writes the change and, when the owners are needed, the
//!   consilium, its seats and their approval mails, all together — through
//!   [`consilium::open_on`] on the same connection — so neither can exist without the other.
//!   On the administrator's path it writes the change already `scheduled` and one notice
//!   per holder in the same transaction, for the reason every governance mail is written
//!   with the fact it announces.
//! - [`FeePolicyChanges::promote`] settles every holder's management accrual at the OLD
//!   rate as of `effective_from` (`carry_accrual` reads `fee_policies` on this same
//!   connection, which is why it runs strictly BEFORE the upsert), then writes the new
//!   terms. `docs/FEES.md` § "The elapsed clock": nobody re-prices time that has passed.
//!
//! Runtime queries throughout (`sqlx::query*`), so `cargo build` needs no database; the
//! integration suite in `tests/fee_policy_changes.rs` executes every one of them.

use async_trait::async_trait;
use domain::{
	balance::ServiceId,
	consilium::ConsiliumId,
	error::DomainError,
	fees::{self, ChangeRequirement, FeePolicy, FeePolicyChangeId, FeePolicyChangeState, FeePolicySubject},
};
use sqlx::{PgConnection, PgPool, Row, postgres::PgRow};
use uuid::Uuid;

use crate::{
	infrastructure::{
		consilium,
		consilium_mailer::{MailSubject, enqueue},
		fee_accrual::carry_accrual,
		fees::{policy_from_row, repo_err},
	},
	ports::{
		fees::{ConsiliumOpening, FeePolicyChange, FeePolicyChanges, NewFeePolicyChange},
		governance_mail::{FeePolicyNotice, FeePolicyTerms, GovernanceMail},
	},
};

/// Postgres' unique-violation SQLSTATE — how `fee_policy_changes_single_pending_idx` answers.
const UNIQUE_VIOLATION: &str = "23505";

macro_rules! change_columns {
	() => {
		"id, service, version, management_bps, performance_bps, hurdle_bps, basis, crystallization, state, requirement, \
		 EXTRACT(EPOCH FROM effective_from)::bigint AS effective_from_unix, consilium_id, requested_by, reason, \
		 EXTRACT(EPOCH FROM requested_at)::bigint AS requested_at_unix, EXTRACT(EPOCH FROM scheduled_at)::bigint AS scheduled_at_unix, \
		 EXTRACT(EPOCH FROM applied_at)::bigint AS applied_at_unix"
	};
}

/// The cabinet-relative path of a product's page, as the holders' notice links to it.
/// Concierge pins it to its own public origin; this plane never mints a host.
pub fn product_page_path(service: &ServiceId) -> String {
	format!("/invest/{service}")
}

pub struct PgFeePolicyChanges {
	pool: PgPool,
}

impl PgFeePolicyChanges {
	pub fn new(pool: PgPool) -> Self {
		Self { pool }
	}
}

/// The five terms as a mail states them.
pub(crate) fn mail_terms(policy: &FeePolicy) -> FeePolicyTerms {
	FeePolicyTerms {
		management_bps: policy.management_bps(),
		performance_bps: policy.performance_bps(),
		hurdle_bps: policy.hurdle_bps(),
		basis: policy.basis().as_str().to_owned(),
		crystallization: policy.crystallization().as_str().to_owned(),
	}
}

/// Take the ONE lock every transaction over a product's terms opens with: its row in
/// `allocations`, `FOR UPDATE`. Scheduling, carrying and promoting a change all read the
/// live terms and write something derived from them (the requirement, the notice's "from",
/// the accrual carried at the old rate); without a common lock, a schedule computing its
/// requirement while a promotion commits reads the terms of a minute ago and records a
/// requirement the new terms would not have allowed. `NotFound` for a slug the registry does
/// not know — terms for a product that does not exist are always a typo.
async fn lock_product(conn: &mut PgConnection, service: &ServiceId) -> Result<(), DomainError> {
	sqlx::query_scalar::<_, String>("SELECT service FROM allocations WHERE service = $1 FOR UPDATE")
		.bind(service.as_str())
		.fetch_optional(&mut *conn)
		.await
		.map_err(repo_err)?
		.map(|_| ())
		.ok_or_else(|| DomainError::NotFound {
			entity: "allocation",
			id: service.to_string(),
		})
}

/// The product's title, or `None` for a slug the registry does not know.
pub(crate) async fn allocation_title(conn: &mut PgConnection, service: &ServiceId) -> Result<Option<String>, DomainError> {
	sqlx::query_scalar::<_, String>("SELECT title FROM allocations WHERE service = $1")
		.bind(service.as_str())
		.fetch_optional(&mut *conn)
		.await
		.map_err(repo_err)
}

/// How many investors hold units of the product right now — who the notice period and the
/// notices are for.
pub(crate) async fn holder_count(conn: &mut PgConnection, service: &ServiceId) -> Result<u32, DomainError> {
	let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM fund_positions WHERE service = $1 AND units <> '0'")
		.bind(service.as_str())
		.fetch_one(&mut *conn)
		.await
		.map_err(repo_err)?;
	Ok(u32::try_from(count).unwrap_or(u32::MAX))
}

/// Close a change whose consilium reached a verdict other than approval, on the verdict's
/// own transaction. A no-op on a change that is not awaiting the owners: the administrator's
/// own cancel reaches the change first and the consilium second, and the second must not
/// fail over the first.
pub(crate) async fn reject_on(conn: &mut PgConnection, change: FeePolicyChangeId, consilium: ConsiliumId, reason: &str) -> Result<(), DomainError> {
	sqlx::query("UPDATE fee_policy_changes SET state = 'rejected', closed_reason = $3 WHERE id = $1 AND consilium_id = $2 AND state = 'awaiting_consilium'")
		.bind(change.raw())
		.bind(consilium.raw())
		.bind(reason)
		.execute(&mut *conn)
		.await
		.map_err(repo_err)?;
	Ok(())
}

fn change_from_row(row: &PgRow) -> Result<FeePolicyChange, DomainError> {
	let (service, policy) = policy_from_row(row)?;
	Ok(FeePolicyChange {
		id: FeePolicyChangeId::from_raw(row.try_get("id").map_err(repo_err)?),
		service,
		version: u32::try_from(row.try_get::<i32, _>("version").map_err(repo_err)?).map_err(|_| DomainError::Repository("negative change version".into()))?,
		policy,
		state: FeePolicyChangeState::parse(row.try_get::<String, _>("state").map_err(repo_err)?.as_str())?,
		requirement: ChangeRequirement::parse(row.try_get::<String, _>("requirement").map_err(repo_err)?.as_str())?,
		effective_from_unix: row.try_get("effective_from_unix").map_err(repo_err)?,
		consilium_id: row.try_get::<Option<Uuid>, _>("consilium_id").map_err(repo_err)?.map(ConsiliumId::from_raw),
		requested_by: row.try_get("requested_by").map_err(repo_err)?,
		requested_at_unix: row.try_get("requested_at_unix").map_err(repo_err)?,
		reason: row.try_get("reason").map_err(repo_err)?,
		scheduled_at_unix: row.try_get("scheduled_at_unix").map_err(repo_err)?,
		applied_at_unix: row.try_get("applied_at_unix").map_err(repo_err)?,
	})
}

async fn find_on(conn: &mut PgConnection, id: FeePolicyChangeId) -> Result<Option<FeePolicyChange>, DomainError> {
	sqlx::query(concat!("SELECT ", change_columns!(), " FROM fee_policy_changes WHERE id = $1"))
		.bind(id.raw())
		.fetch_optional(&mut *conn)
		.await
		.map_err(repo_err)?
		.as_ref()
		.map(change_from_row)
		.transpose()
}

/// Load a change `FOR UPDATE` — the opening move of every transition on it. LOCK ORDER: when
/// a consilium row is involved it is locked FIRST (see [`FeePolicyChanges::cancel`]), the
/// order every consilium transition that cascades onto a change already uses.
async fn locked(conn: &mut PgConnection, id: FeePolicyChangeId) -> Result<FeePolicyChange, DomainError> {
	sqlx::query(concat!("SELECT ", change_columns!(), " FROM fee_policy_changes WHERE id = $1 FOR UPDATE"))
		.bind(id.raw())
		.fetch_optional(&mut *conn)
		.await
		.map_err(repo_err)?
		.as_ref()
		.map(change_from_row)
		.transpose()?
		.ok_or_else(|| DomainError::NotFound {
			entity: "fee policy change",
			id: id.to_string(),
		})
}

/// The terms in force for a product right now, or `None` for one that charges nothing —
/// read on the caller's connection so a notice written in a transaction states the row that
/// transaction sees.
async fn current_terms(conn: &mut PgConnection, service: &ServiceId) -> Result<Option<FeePolicy>, DomainError> {
	let row = sqlx::query("SELECT service, management_bps, performance_bps, hurdle_bps, basis, crystallization FROM fee_policies WHERE service = $1")
		.bind(service.as_str())
		.fetch_optional(&mut *conn)
		.await
		.map_err(repo_err)?;
	row.map(|row| policy_from_row(&row).map(|(_, policy)| policy)).transpose()
}

/// One holder of the product: their banking id and, when mirrored, their identity-plane id.
struct Holder {
	user_id: Uuid,
	concierge_user_id: Option<Uuid>,
}

async fn holders(conn: &mut PgConnection, service: &ServiceId) -> Result<Vec<Holder>, DomainError> {
	let rows = sqlx::query("SELECT p.user_id, u.concierge_user_id FROM fund_positions p JOIN users u ON u.id = p.user_id WHERE p.service = $1 AND p.units <> '0' ORDER BY p.user_id")
		.bind(service.as_str())
		.fetch_all(&mut *conn)
		.await
		.map_err(repo_err)?;
	rows.iter()
		.map(|row| {
			Ok(Holder {
				user_id: row.try_get("user_id").map_err(repo_err)?,
				concierge_user_id: row.try_get("concierge_user_id").map_err(repo_err)?,
			})
		})
		.collect()
}

/// Queue one notice per holder, on the caller's transaction, keyed
/// `fee-policy-notice:<change>:<user>` so a retried scheduling enqueues each exactly once.
///
/// A holder with no mirrored identity-plane id is still queued, with an empty
/// `subject_user_id`: the worker retires that row loudly ("recipient has no mirrored
/// concierge user id") rather than this path silently skipping someone the notice period
/// exists to protect.
async fn enqueue_notices(conn: &mut PgConnection, change: &FeePolicyChange, from: Option<&FeePolicy>, holders: &[Holder]) -> Result<(), DomainError> {
	let fund = allocation_title(conn, &change.service)
		.await?
		.filter(|title| !title.is_empty())
		.unwrap_or_else(|| change.service.to_string());
	let link = product_page_path(&change.service);
	for holder in holders {
		let mail = GovernanceMail::FeePolicyNotice(FeePolicyNotice {
			subject_user_id: holder.concierge_user_id.map(|id| id.to_string()).unwrap_or_default(),
			fund: fund.clone(),
			current: from.map(mail_terms),
			proposed: mail_terms(&change.policy),
			effective_at: change.effective_from_unix,
			link: link.clone(),
		});
		let key = format!("fee-policy-notice:{}:{}", change.id, holder.user_id);
		enqueue(conn, MailSubject::FeePolicyChange(change.id.raw()), holder.user_id, &key, &mail).await?;
	}
	Ok(())
}

fn bps(value: u32) -> i32 {
	// The constructor caps every rate far below `i32::MAX`; the fallback only guards the cast.
	i32::try_from(value).unwrap_or(i32::MAX)
}

#[async_trait]
impl FeePolicyChanges for PgFeePolicyChanges {
	async fn schedule(&self, change: &NewFeePolicyChange, consilium: Option<ConsiliumOpening<'_>>) -> Result<FeePolicyChange, DomainError> {
		let mut tx = self.pool.begin().await.map_err(repo_err)?;
		lock_product(&mut tx, &change.service).await?;
		let from = current_terms(&mut tx, &change.service).await?;
		// The application decided the requirement against the terms it read BEFORE this lock.
		// A promotion may have committed in between (an INSERT waiting on the pending index
		// is exactly that window), so the decision is re-taken under the lock and any
		// disagreement is a refusal: the caller re-reads and re-submits, rather than this path
		// recording a requirement the live terms would not have allowed.
		let stale = || DomainError::Conflict("the live terms changed while the request was being recorded — re-submit".into());
		if fees::requirement_for(from.as_ref(), &change.policy) != change.requirement {
			return Err(stale());
		}
		if let Some(opening) = &consilium
			&& let domain::consilium::ConsiliumTerms::FeePolicy(subject) = opening.consilium.terms()
			&& subject.from != from
		{
			// The owners would be signing a "from" that is no longer the truth.
			return Err(stale());
		}
		let holders = holders(&mut tx, &change.service).await?;
		let version: i32 = sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) + 1 FROM fee_policy_changes WHERE service = $1")
			.bind(change.service.as_str())
			.fetch_one(&mut *tx)
			.await
			.map_err(repo_err)?;

		// The consilium first: it is the FK target of the change's `consilium_id`, and its
		// own single-open index may refuse (another fee-policy consilium is open over this
		// product), in which case nothing else must be written either.
		let (state, consilium_id, scheduled_at, effective_from) = match consilium {
			None => (
				FeePolicyChangeState::Scheduled,
				None,
				Some(change.now_unix),
				fees::earliest_effective_from(change.now_unix, change.requested_effective_from_unix, !holders.is_empty()),
			),
			Some(opening) => {
				consilium::open_on(&mut tx, opening.consilium, opening.credentials, opening.approval_url_base).await?;
				// Provisional: the notice clock starts when the owners carry the change.
				(
					FeePolicyChangeState::AwaitingConsilium,
					Some(opening.consilium.id()),
					None,
					change.requested_effective_from_unix.max(change.now_unix),
				)
			}
		};

		let inserted = sqlx::query(
			"INSERT INTO fee_policy_changes (id, service, version, management_bps, performance_bps, hurdle_bps, basis, crystallization, \
			   state, requirement, effective_from, requested_by, requested_at, reason, consilium_id, scheduled_at) \
			 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, to_timestamp($11), $12, to_timestamp($13), $14, $15, to_timestamp($16))",
		)
		.bind(change.id.raw())
		.bind(change.service.as_str())
		.bind(version)
		.bind(bps(change.policy.management_bps()))
		.bind(bps(change.policy.performance_bps()))
		.bind(bps(change.policy.hurdle_bps()))
		.bind(change.policy.basis().as_str())
		.bind(change.policy.crystallization().as_str())
		.bind(state.as_str())
		.bind(change.requirement.as_str())
		.bind(effective_from as f64)
		.bind(&change.requested_by)
		.bind(change.now_unix as f64)
		.bind(&change.reason)
		.bind(consilium_id.map(|id| id.raw()))
		.bind(scheduled_at.map(|at| at as f64))
		.execute(&mut *tx)
		.await;
		if let Err(sqlx::Error::Database(err)) = &inserted
			&& err.code().as_deref() == Some(UNIQUE_VIOLATION)
		{
			// The pending index (or, on a concurrent request, the version key) spoke: one change
			// on its way per product, so the second is a refusal, not a retry.
			return Err(DomainError::Conflict(
				"a fee-policy change is already pending for this product — cancel it before scheduling another".into(),
			));
		}
		inserted.map_err(repo_err)?;

		let stored = find_on(&mut tx, change.id)
			.await?
			.ok_or_else(|| DomainError::Repository("fee policy change vanished inside its own transaction".into()))?;
		if state == FeePolicyChangeState::Scheduled {
			enqueue_notices(&mut tx, &stored, from.as_ref(), &holders).await?;
		}
		tx.commit().await.map_err(repo_err)?;
		Ok(stored)
	}

	async fn schedule_approved(&self, subject: &FeePolicySubject, consilium: ConsiliumId, now_unix: i64) -> Result<FeePolicyChange, DomainError> {
		let mut tx = self.pool.begin().await.map_err(repo_err)?;
		lock_product(&mut tx, &subject.service).await?;
		let change = locked(&mut tx, subject.change_id).await?;
		if change.consilium_id != Some(consilium) || change.requirement != ChangeRequirement::OwnerConsilium {
			return Err(DomainError::Conflict("this consilium does not decide this fee-policy change".into()));
		}
		// The row is re-checked against the subject the owners signed, as a payment order is
		// re-hashed against its consilium: a change edited underneath its quorum cannot spend
		// the quorum's signature.
		if change.service != subject.service || change.policy != subject.to || change.reason != subject.reason {
			return Err(DomainError::Conflict("the fee-policy change no longer matches the terms the owners approved".into()));
		}
		match change.state {
			// The retry an at-least-once execution path depends on.
			FeePolicyChangeState::Scheduled => return Ok(change),
			FeePolicyChangeState::AwaitingConsilium => {}
			state @ (FeePolicyChangeState::Active | FeePolicyChangeState::Superseded | FeePolicyChangeState::Rejected | FeePolicyChangeState::Cancelled) => {
				return Err(DomainError::Conflict(format!("the fee-policy change is {}, no longer awaiting the owners", state.as_str())));
			}
		}
		let from = current_terms(&mut tx, &change.service).await?;
		let holders = holders(&mut tx, &change.service).await?;
		let effective_from = fees::earliest_effective_from(now_unix, subject.requested_effective_from, !holders.is_empty());
		sqlx::query("UPDATE fee_policy_changes SET state = 'scheduled', scheduled_at = to_timestamp($2), effective_from = to_timestamp($3) WHERE id = $1")
			.bind(change.id.raw())
			.bind(now_unix as f64)
			.bind(effective_from as f64)
			.execute(&mut *tx)
			.await
			.map_err(repo_err)?;
		let stored = find_on(&mut tx, change.id)
			.await?
			.ok_or_else(|| DomainError::Repository("fee policy change vanished under lock".into()))?;
		enqueue_notices(&mut tx, &stored, from.as_ref(), &holders).await?;
		tx.commit().await.map_err(repo_err)?;
		Ok(stored)
	}

	async fn cancel(&self, service: &ServiceId, id: FeePolicyChangeId, by: &str, now_unix: i64) -> Result<FeePolicyChange, DomainError> {
		let mut tx = self.pool.begin().await.map_err(repo_err)?;
		// An unlocked probe for the consilium, so the CONSILIUM row can be locked first — the
		// order every consilium transition that cascades onto this table already uses. The
		// change is then re-read under its own lock, so nothing below trusts the probe.
		let probe = find_on(&mut tx, id).await?.ok_or_else(|| DomainError::NotFound {
			entity: "fee policy change",
			id: id.to_string(),
		})?;
		if let (Some(consilium), true) = (probe.consilium_id, probe.state == FeePolicyChangeState::AwaitingConsilium) {
			consilium::withdraw_on(&mut tx, consilium, now_unix).await?;
		}
		let change = locked(&mut tx, id).await?;
		if &change.service != service {
			return Err(DomainError::NotFound {
				entity: "fee policy change",
				id: id.to_string(),
			});
		}
		match change.state {
			FeePolicyChangeState::Cancelled => return Ok(change),
			// `withdraw_on` above may already have closed it as `rejected` through the
			// consilium's cascade; either way it is being withdrawn by an administrator now,
			// and that is the state the history should say.
			FeePolicyChangeState::Scheduled | FeePolicyChangeState::AwaitingConsilium | FeePolicyChangeState::Rejected => {}
			state @ (FeePolicyChangeState::Active | FeePolicyChangeState::Superseded) => {
				return Err(DomainError::Conflict(format!("the fee-policy change is {} and can no longer be cancelled", state.as_str())));
			}
		}
		sqlx::query("UPDATE fee_policy_changes SET state = 'cancelled', closed_reason = $2 WHERE id = $1")
			.bind(id.raw())
			.bind(format!("withdrawn by {by}"))
			.execute(&mut *tx)
			.await
			.map_err(repo_err)?;
		let stored = find_on(&mut tx, id)
			.await?
			.ok_or_else(|| DomainError::Repository("fee policy change vanished under lock".into()))?;
		tx.commit().await.map_err(repo_err)?;
		Ok(stored)
	}

	async fn find(&self, id: FeePolicyChangeId) -> Result<Option<FeePolicyChange>, DomainError> {
		let mut conn = self.pool.acquire().await.map_err(repo_err)?;
		find_on(&mut conn, id).await
	}

	async fn holder_count(&self, service: &ServiceId) -> Result<u32, DomainError> {
		let mut conn = self.pool.acquire().await.map_err(repo_err)?;
		holder_count(&mut conn, service).await
	}

	async fn pending(&self, service: &ServiceId) -> Result<Option<FeePolicyChange>, DomainError> {
		sqlx::query(concat!(
			"SELECT ",
			change_columns!(),
			" FROM fee_policy_changes WHERE service = $1 AND state IN ('awaiting_consilium', 'scheduled')"
		))
		.bind(service.as_str())
		.fetch_optional(&self.pool)
		.await
		.map_err(repo_err)?
		.as_ref()
		.map(change_from_row)
		.transpose()
	}

	async fn list(&self, service: &ServiceId) -> Result<Vec<FeePolicyChange>, DomainError> {
		let rows = sqlx::query(concat!("SELECT ", change_columns!(), " FROM fee_policy_changes WHERE service = $1 ORDER BY version DESC"))
			.bind(service.as_str())
			.fetch_all(&self.pool)
			.await
			.map_err(repo_err)?;
		rows.iter().map(change_from_row).collect()
	}

	async fn due(&self, now_unix: i64) -> Result<Vec<FeePolicyChangeId>, DomainError> {
		let ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM fee_policy_changes WHERE state = 'scheduled' AND effective_from <= to_timestamp($1) ORDER BY effective_from")
			.bind(now_unix)
			.fetch_all(&self.pool)
			.await
			.map_err(repo_err)?;
		Ok(ids.into_iter().map(FeePolicyChangeId::from_raw).collect())
	}

	async fn promote(&self, id: FeePolicyChangeId, now_unix: i64) -> Result<bool, DomainError> {
		let mut tx = self.pool.begin().await.map_err(repo_err)?;
		// An unlocked probe for the product, so the PRODUCT lock is taken before the change's —
		// the order `schedule` and `schedule_approved` use. Nothing below trusts the probe.
		let Some(probe) = find_on(&mut tx, id).await? else {
			return Ok(false);
		};
		lock_product(&mut tx, &probe.service).await?;
		let change = locked(&mut tx, id).await?;
		if change.state != FeePolicyChangeState::Scheduled || change.effective_from_unix > now_unix {
			return Ok(false);
		}

		// Every holder's row, locked — the same lock a charge and a settle take — so no
		// assessment can read the new terms against a clock the old terms still own.
		let positions =
			sqlx::query("SELECT user_id, EXTRACT(EPOCH FROM fees_accrued_at)::bigint AS accrued_at_unix FROM fund_positions WHERE service = $1 AND units <> '0' ORDER BY user_id FOR UPDATE")
				.bind(change.service.as_str())
				.fetch_all(&mut *tx)
				.await
				.map_err(repo_err)?;
		for row in &positions {
			let user: Uuid = row.try_get("user_id").map_err(repo_err)?;
			let accrued_at: i64 = row.try_get("accrued_at_unix").map_err(repo_err)?;
			// A clock already past `effective_from` (a sweep or a top-up landed in the minute
			// between the moment and this promotion) has nothing left to settle at the old
			// rate; carrying it would only move the clock backwards.
			if accrued_at < change.effective_from_unix {
				carry_accrual(&mut tx, user, change.service.as_str(), change.effective_from_unix).await?;
			}
		}

		// STRICTLY AFTER the carries above: `carry_accrual` prices the elapsed window off the
		// row this upsert replaces.
		sqlx::query(
			"INSERT INTO fee_policies (service, management_bps, performance_bps, hurdle_bps, basis, crystallization, updated_by, updated_at, version, effective_from) \
			 VALUES ($1, $2, $3, $4, $5, $6, $7, now(), $8, to_timestamp($9)) \
			 ON CONFLICT (service) DO UPDATE SET management_bps = EXCLUDED.management_bps, performance_bps = EXCLUDED.performance_bps, \
			   hurdle_bps = EXCLUDED.hurdle_bps, basis = EXCLUDED.basis, crystallization = EXCLUDED.crystallization, \
			   updated_by = EXCLUDED.updated_by, updated_at = now(), version = EXCLUDED.version, effective_from = EXCLUDED.effective_from",
		)
		.bind(change.service.as_str())
		.bind(bps(change.policy.management_bps()))
		.bind(bps(change.policy.performance_bps()))
		.bind(bps(change.policy.hurdle_bps()))
		.bind(change.policy.basis().as_str())
		.bind(change.policy.crystallization().as_str())
		.bind(&change.requested_by)
		.bind(i32::try_from(change.version).unwrap_or(i32::MAX))
		.bind(change.effective_from_unix as f64)
		.execute(&mut *tx)
		.await
		.map_err(repo_err)?;

		sqlx::query("UPDATE fee_policy_changes SET state = 'superseded' WHERE service = $1 AND state = 'active'")
			.bind(change.service.as_str())
			.execute(&mut *tx)
			.await
			.map_err(repo_err)?;
		sqlx::query("UPDATE fee_policy_changes SET state = 'active', applied_at = to_timestamp($2) WHERE id = $1")
			.bind(id.raw())
			.bind(now_unix as f64)
			.execute(&mut *tx)
			.await
			.map_err(repo_err)?;
		tx.commit().await.map_err(repo_err)?;
		Ok(true)
	}
}
