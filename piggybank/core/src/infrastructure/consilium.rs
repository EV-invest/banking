//! Postgres adapter for the [`ConsiliumRepository`] port.
//!
//! The load-bearing method is [`ConsiliumRepository::submit`]: the attempt counter, the code
//! comparison, the tally and the state transition all happen inside ONE transaction holding
//! `SELECT … FOR UPDATE` on the consilium row. Counting the attempt *before* comparing (and
//! in the same transaction) is what stops concurrent guesses slipping past the five-attempt
//! limit; taking the verdict under the same lock is what stops two voters both reading a
//! sub-threshold tally and both concluding they were not the deciding vote.
//!
//! Consilium events are audit-only: they land in `event_log` with `relay = false`, never in
//! the `outbox`. A consilium moves no money, so the relay has no ledger op for the kind.
//!
//! Runtime queries (`sqlx::query*`, never the compile-time macros) keep `cargo build`
//! independent of a live database; sqlx 0.9 takes only a `&'static str`, so every column
//! list is spliced in with `concat!` rather than built with `format!`.

use std::collections::HashMap;

use async_trait::async_trait;
use domain::{
	consilium::{Consilium, ConsiliumId, ConsiliumKind, ConsiliumState, ConsiliumVote, RevenuePayoutTerms, VoteDecision},
	error::DomainError,
	money::{Network, Usdt, WalletAddress},
	users::UserId,
	withdrawals::WithdrawalId,
};
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, PgPool, Row, postgres::PgRow};
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::{
	infrastructure::outbox,
	ports::{
		consilium::{
			ConsiliumRepository, ConsiliumView, DIGEST_BYTES, ExecutionOutcome, InvitationView, MAX_CODE_ATTEMPTS, SubmitOutcome, VoteAudit, VoterCredential, VoterView, invitation_not_found,
		},
		governance_mail::{GovernanceMail, PayoutApproval, PayoutOutcome},
	},
};

/// Postgres' unique-violation SQLSTATE — how the single-open-consilium index answers.
const UNIQUE_VIOLATION: &str = "23505";

macro_rules! consilium_columns {
	() => {
		"c.id, c.kind, c.state, c.terms::text AS terms, c.payload_hash, c.initiator_user_id, c.owner_count, c.threshold, \
		 EXTRACT(EPOCH FROM c.created_at)::bigint AS created_at, EXTRACT(EPOCH FROM c.expires_at)::bigint AS expires_at, \
		 EXTRACT(EPOCH FROM c.decided_at)::bigint AS decided_at, c.executed_withdrawal_id, c.failure_reason, c.version"
	};
}

/// A vote counts only while its caster is an owner in good standing. A disabled or frozen
/// account is not a principal that may authorize money leaving the fund, and dropping such a
/// vote can only ever make approval harder — never easier — which is the direction every
/// roster rule here is required to move in.
macro_rules! seat_columns {
	() => {
		"v.user_id, u.email, v.decision, EXTRACT(EPOCH FROM v.decided_at)::bigint AS decided_at, v.notified, v.attempts, \
		 (u.role = 'owner' AND NOT u.frozen AND u.status <> 'disabled') AS still_an_owner"
	};
}

macro_rules! seats_query {
	() => {
		concat!(
			"SELECT ",
			seat_columns!(),
			" FROM consilium_voter v JOIN users u ON u.id = v.user_id WHERE v.consilium_id = $1 ORDER BY v.user_id"
		)
	};
}

/// The same seat projection for MANY consilia at once, carrying `consilium_id` so the caller
/// can group. `list` used to run one seats query and one email query per row — 401 round
/// trips for 200 consilia — which is a page load an operator feels.
macro_rules! seats_of_many_query {
	() => {
		concat!(
			"SELECT v.consilium_id, ",
			seat_columns!(),
			" FROM consilium_voter v JOIN users u ON u.id = v.user_id WHERE v.consilium_id = ANY($1) ORDER BY v.consilium_id, v.user_id"
		)
	};
}

macro_rules! token_query {
	() => {
		concat!(
			"SELECT ",
			consilium_columns!(),
			", v.consilium_id, v.user_id, u.email AS voter_email, v.decision AS seat_decision, v.code_hash, v.attempts AS seat_attempts, \
			 v.burned_at IS NOT NULL AS burned, v.used_at IS NOT NULL AS used, EXTRACT(EPOCH FROM v.expires_at)::bigint AS token_expires_at \
			 FROM consilium_voter v JOIN consilium c ON c.id = v.consilium_id JOIN users u ON u.id = v.user_id WHERE v.token_hash = $1"
		)
	};
}

pub struct PgConsilia {
	pool: PgPool,
}

impl PgConsilia {
	pub fn new(pool: PgPool) -> Self {
		Self { pool }
	}

	/// Close one consilium whose window ran out. `false` means a vote carried it between the
	/// probe and the lock, which is not a failure.
	async fn expire_one(&self, id: Uuid, at: i64) -> Result<bool, DomainError> {
		let mut tx = self.pool.begin().await.map_err(repo_err)?;
		let (mut consilium, _) = locked(&mut tx, ConsiliumId::from_raw(id)).await?;
		if !consilium.state().is_open() {
			return Ok(false);
		}
		consilium.expire(at)?;
		persist(&mut tx, &mut consilium).await?;
		announce(&mut tx, &consilium, "the window closed with no verdict").await?;
		tx.commit().await.map_err(repo_err)?;
		Ok(true)
	}

	/// Void one consilium that was open across a roster change. Cancellation is the existing
	/// terminal state for "this request is no longer valid"; the audience is told why so the
	/// delay reads as the deliberate control it is rather than an unexplained disappearance.
	async fn void_one(&self, id: Uuid, at: i64) -> Result<bool, DomainError> {
		let mut tx = self.pool.begin().await.map_err(repo_err)?;
		let (mut consilium, _) = locked(&mut tx, ConsiliumId::from_raw(id)).await?;
		if !consilium.state().is_open() {
			return Ok(false);
		}
		consilium.cancel(at)?;
		persist(&mut tx, &mut consilium).await?;
		announce(
			&mut tx,
			&consilium,
			"the owner roster changed while this request was open, so it was voided; it may be proposed again after the cooling-off period",
		)
		.await?;
		tx.commit().await.map_err(repo_err)?;
		Ok(true)
	}
}

/// The stored JSONB shape of the terms. Amounts are exact base-unit strings, as everywhere
/// else in the control plane — Postgres never holds a money figure it reasons about.
#[derive(Deserialize, Serialize)]
struct StoredTerms {
	network: String,
	address: String,
	amount: String,
	memo: String,
}

impl StoredTerms {
	fn of(terms: &RevenuePayoutTerms) -> Self {
		Self {
			network: terms.network.as_str().to_owned(),
			address: terms.address.as_str().to_owned(),
			amount: terms.amount.base_units().to_string(),
			memo: terms.memo.clone(),
		}
	}

	fn into_domain(self) -> Result<RevenuePayoutTerms, DomainError> {
		let network = Network::parse(&self.network)?;
		let address = WalletAddress::parse(network, &self.address)?;
		let amount = Usdt::from_base_units(self.amount.parse::<u128>().map_err(|_| DomainError::Repository("malformed consilium amount".into()))?);
		RevenuePayoutTerms::new(network, address, amount, self.memo)
	}
}

fn repo_err(err: sqlx::Error) -> DomainError {
	DomainError::Repository(err.to_string())
}

pub(crate) fn digest(bytes: &[u8]) -> [u8; DIGEST_BYTES] {
	use sha2::{Digest, Sha256};
	Sha256::digest(bytes).into()
}

/// Compare two digests without leaking, through timing, how much of a guess was right.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
	a.len() == b.len() && a.ct_eq(b).into()
}

/// One seat, joined to the live roster so the tally can tell a vote that still counts from
/// one whose owner has since lost their seat.
struct SeatRow {
	user_id: Uuid,
	email: String,
	decision: String,
	decided_at: Option<i64>,
	notified: bool,
	still_an_owner: bool,
}

fn seat_of(row: &PgRow) -> Result<SeatRow, DomainError> {
	Ok(SeatRow {
		user_id: row.try_get("user_id").map_err(repo_err)?,
		email: row.try_get("email").map_err(repo_err)?,
		decision: row.try_get("decision").map_err(repo_err)?,
		decided_at: row.try_get("decided_at").map_err(repo_err)?,
		notified: row.try_get("notified").map_err(repo_err)?,
		still_an_owner: row.try_get("still_an_owner").map_err(repo_err)?,
	})
}

async fn seats_of(conn: &mut PgConnection, id: Uuid) -> Result<Vec<SeatRow>, DomainError> {
	let rows = sqlx::query(seats_query!()).bind(id).fetch_all(&mut *conn).await.map_err(repo_err)?;
	rows.iter().map(seat_of).collect()
}

async fn seats_of_locked(conn: &mut PgConnection, id: Uuid) -> Result<Vec<SeatRow>, DomainError> {
	let rows = sqlx::query(concat!(seats_query!(), " FOR UPDATE OF v")).bind(id).fetch_all(&mut *conn).await.map_err(repo_err)?;
	rows.iter().map(seat_of).collect()
}

/// Rebuild the aggregate from a consilium row plus its seats.
///
/// The `counts` flag is the roster re-check that closes the quorum-lowering hole: a vote is
/// carried into the tally only if its caster is STILL an owner. The snapshot alone is not
/// trusted, so losing a seat retroactively voids the vote cast from it.
fn rehydrate(row: &PgRow, seats: &[SeatRow]) -> Result<Consilium, DomainError> {
	let stored: StoredTerms = serde_json::from_str(row.try_get::<String, _>("terms").map_err(repo_err)?.as_str()).map_err(|e| DomainError::Repository(e.to_string()))?;
	let hash_bytes: Vec<u8> = row.try_get("payload_hash").map_err(repo_err)?;
	let payload_hash: [u8; DIGEST_BYTES] = hash_bytes
		.try_into()
		.map_err(|_| DomainError::Repository("consilium payload_hash is not a sha-256 digest".into()))?;
	let eligible = seats.iter().map(|seat| UserId::from_raw(seat.user_id)).collect();
	let votes = seats
		.iter()
		.filter(|seat| seat.decision != VoteDecision::Pending.as_str())
		.map(|seat| {
			Ok(ConsiliumVote {
				voter: UserId::from_raw(seat.user_id),
				decision: VoteDecision::parse(&seat.decision)?,
				decided_at: seat.decided_at.unwrap_or_default(),
				counts: seat.still_an_owner,
			})
		})
		.collect::<Result<Vec<_>, DomainError>>()?;
	Ok(Consilium::rehydrate(
		ConsiliumId::from_raw(row.try_get("id").map_err(repo_err)?),
		ConsiliumKind::parse(row.try_get::<String, _>("kind").map_err(repo_err)?.as_str())?,
		stored.into_domain()?,
		payload_hash,
		UserId::from_raw(row.try_get("initiator_user_id").map_err(repo_err)?),
		row.try_get::<i32, _>("owner_count").map_err(repo_err)? as u32,
		row.try_get::<i32, _>("threshold").map_err(repo_err)? as u32,
		eligible,
		votes,
		ConsiliumState::parse(row.try_get::<String, _>("state").map_err(repo_err)?.as_str())?,
		row.try_get("created_at").map_err(repo_err)?,
		row.try_get("expires_at").map_err(repo_err)?,
		row.try_get("decided_at").map_err(repo_err)?,
		row.try_get::<Option<Uuid>, _>("executed_withdrawal_id").map_err(repo_err)?.map(WithdrawalId::from_raw),
		row.try_get("failure_reason").map_err(repo_err)?,
		row.try_get::<i64, _>("version").map_err(repo_err)? as u64,
	))
}

/// The initiator's address. A missing row is an error, not an empty string: every consilium
/// has an initiator by FK, so `None` here means the projection is broken — and returning
/// `""` would let a governance mail go out addressed to nobody while every layer above
/// reported success.
async fn email_of(conn: &mut PgConnection, id: UserId) -> Result<String, DomainError> {
	sqlx::query_scalar::<_, String>("SELECT email FROM users WHERE id = $1")
		.bind(id.raw())
		.fetch_optional(&mut *conn)
		.await
		.map_err(repo_err)?
		.ok_or_else(|| DomainError::Repository(format!("consilium initiator {id} has no mirrored user row")))
}

/// A decision that will not parse is a corrupt row, and rendering it as `Pending` would show
/// an operator a seat that has not answered when it may well have approved — the one figure
/// on this screen that must never be understated. The CHECK constraint makes it
/// unreachable; if it ever happens, it surfaces.
fn view_of(consilium: Consilium, initiator_email: String, seats: &[SeatRow]) -> Result<ConsiliumView, DomainError> {
	let voters = seats
		.iter()
		.map(|seat| {
			Ok(VoterView {
				user_id: UserId::from_raw(seat.user_id),
				email: seat.email.clone(),
				decision: VoteDecision::parse(&seat.decision)?,
				decided_at: seat.decided_at.unwrap_or_default(),
				notified: seat.notified,
			})
		})
		.collect::<Result<Vec<_>, DomainError>>()?;
	Ok(ConsiliumView { consilium, initiator_email, voters })
}

/// Persist a transition and drain its events. The version written is the aggregate's own, so
/// the live page's "refetch when the version moves" contract rides the counter the domain
/// bumps rather than a second one maintained here.
async fn persist(conn: &mut PgConnection, consilium: &mut Consilium) -> Result<(), DomainError> {
	let affected = sqlx::query("UPDATE consilium SET state = $2, decided_at = to_timestamp($3), executed_withdrawal_id = $4, failure_reason = $5, version = $6 WHERE id = $1")
		.bind(consilium.id().raw())
		.bind(consilium.state().as_str())
		.bind(consilium.decided_at().map(|at| at as f64))
		.bind(consilium.executed_withdrawal_id().map(|id| id.raw()))
		.bind(consilium.failure_reason())
		.bind(consilium.version() as i64)
		.execute(&mut *conn)
		.await
		.map_err(repo_err)?
		.rows_affected();
	if affected != 1 {
		return Err(DomainError::Repository("consilium row vanished under lock".into()));
	}
	outbox::drain_to_outbox(conn, consilium, false).await
}

/// Queue one mail. `ON CONFLICT DO NOTHING` on the dedupe key makes a retried transition (or
/// a redelivered sweep) enqueue the same notification exactly once.
async fn enqueue(conn: &mut PgConnection, consilium_id: Uuid, user_id: Uuid, dedupe_key: &str, mail: &GovernanceMail) -> Result<(), DomainError> {
	let payload = serde_json::to_string(mail).map_err(|e| DomainError::Repository(e.to_string()))?;
	sqlx::query("INSERT INTO consilium_mail (consilium_id, user_id, kind, dedupe_key, payload) VALUES ($1, $2, $3, $4, $5::jsonb) ON CONFLICT (dedupe_key) DO NOTHING")
		.bind(consilium_id)
		.bind(user_id)
		.bind(mail.as_str())
		.bind(dedupe_key)
		.bind(payload)
		.execute(&mut *conn)
		.await
		.map_err(repo_err)?;
	Ok(())
}

/// Everyone who was told about this consilium: the seats, plus the initiator — who opened it
/// and is entitled to learn how it ended without polling for the answer.
fn audience(consilium: &Consilium) -> impl Iterator<Item = UserId> + '_ {
	core::iter::once(consilium.initiator()).chain(consilium.eligible().iter().copied())
}

/// Tell that audience how the consilium ended.
async fn announce(conn: &mut PgConnection, consilium: &Consilium, detail: &str) -> Result<(), DomainError> {
	let mail = GovernanceMail::PayoutOutcome(PayoutOutcome {
		consilium_id: consilium.id().to_string(),
		outcome: consilium.state().as_str().to_uppercase(),
		network: consilium.terms().network.as_str().to_owned(),
		address: consilium.terms().address.as_str().to_owned(),
		amount: consilium.terms().amount.to_decimal_string(),
		detail: detail.to_owned(),
	});
	for recipient in audience(consilium) {
		let key = format!("consilium:{}:outcome:{}:{recipient}", consilium.id(), consilium.state().as_str());
		enqueue(conn, consilium.id().raw(), recipient.raw(), &key, &mail).await?;
	}
	Ok(())
}

/// Tell that audience a token burned. A brute-force attempt against one seat is a fact the
/// whole roster needs, not just its holder — who may be the one person who never sees it.
async fn announce_burn(conn: &mut PgConnection, consilium: &Consilium, voter: UserId) -> Result<(), DomainError> {
	let mail = GovernanceMail::TokenBurned(PayoutOutcome {
		consilium_id: consilium.id().to_string(),
		outcome: "TOKEN_BURNED".to_owned(),
		network: consilium.terms().network.as_str().to_owned(),
		address: consilium.terms().address.as_str().to_owned(),
		amount: consilium.terms().amount.to_decimal_string(),
		detail: format!("five failed code attempts burned the approval token for seat {voter}"),
	});
	for recipient in audience(consilium) {
		let key = format!("consilium:{}:burn:{voter}:{recipient}", consilium.id());
		enqueue(conn, consilium.id().raw(), recipient.raw(), &key, &mail).await?;
	}
	Ok(())
}

/// A mistyped code, told apart from a dead link.
///
/// This is `Validation` (→ gRPC `INVALID_ARGUMENT` → HTTP 400) and NOT `Forbidden`
/// (→ `PERMISSION_DENIED`), which the BFF folds into the same opaque 404 it gives an unknown
/// token. Under that mapping the plane's careful "incorrect code — N attempts remaining"
/// never reached the human: an owner mistyped, was told the invitation did not exist, tried
/// four more times to be sure, burned their own token — and the burn notice then mailed the
/// entire roster a brute-force alert about a legitimate owner.
///
/// This does NOT weaken the enumeration protection in [`invitation_not_found`]. That protects
/// the TOKEN: unknown, expired, spent and burned stay indistinguishable from one another.
/// Someone holding a valid, live token already knows it exists — telling them their CODE was
/// wrong reveals nothing they could not confirm by reading the invitation page, and the five
/// attempt ceiling is what actually guards the code.
fn wrong_code(message: String) -> DomainError {
	DomainError::Validation(message)
}

/// Load a consilium `FOR UPDATE` with its seats — the opening move of every transition here.
/// The consilium row is always locked first, so the transitions cannot deadlock each other.
async fn locked(conn: &mut PgConnection, id: ConsiliumId) -> Result<(Consilium, Vec<SeatRow>), DomainError> {
	let row = sqlx::query(concat!("SELECT ", consilium_columns!(), " FROM consilium c WHERE c.id = $1 FOR UPDATE"))
		.bind(id.raw())
		.fetch_optional(&mut *conn)
		.await
		.map_err(repo_err)?
		.ok_or_else(|| DomainError::NotFound {
			entity: "consilium",
			id: id.to_string(),
		})?;
	let seats = seats_of_locked(conn, id.raw()).await?;
	let consilium = rehydrate(&row, &seats)?;
	Ok((consilium, seats))
}

/// Everything one token resolves to: the seat, and the consilium row behind it.
struct TokenSeat {
	consilium: PgRow,
	consilium_id: Uuid,
	user_id: Uuid,
	email: String,
	decision: String,
	code_hash: Vec<u8>,
	attempts: i32,
	burned: bool,
	used: bool,
	token_expires_at: i64,
}

fn token_seat_of(row: PgRow) -> Result<TokenSeat, DomainError> {
	Ok(TokenSeat {
		consilium_id: row.try_get("consilium_id").map_err(repo_err)?,
		user_id: row.try_get("user_id").map_err(repo_err)?,
		email: row.try_get("voter_email").map_err(repo_err)?,
		decision: row.try_get("seat_decision").map_err(repo_err)?,
		code_hash: row.try_get("code_hash").map_err(repo_err)?,
		attempts: row.try_get("seat_attempts").map_err(repo_err)?,
		burned: row.try_get("burned").map_err(repo_err)?,
		used: row.try_get("used").map_err(repo_err)?,
		token_expires_at: row.try_get("token_expires_at").map_err(repo_err)?,
		consilium: row,
	})
}

fn invitation_of(consilium: &Consilium, initiator_email: String, seat: &TokenSeat, decision: VoteDecision) -> InvitationView {
	InvitationView {
		consilium_id: consilium.id(),
		state: consilium.state(),
		terms: consilium.terms().clone(),
		payload_hash: consilium.payload_hash_hex(),
		initiator_email,
		voter_email: seat.email.clone(),
		threshold: consilium.threshold(),
		approvals: consilium.approvals(),
		owner_count: consilium.owner_count(),
		created_at: consilium.created_at(),
		expires_at: consilium.expires_at(),
		decision,
		attempts_remaining: (MAX_CODE_ATTEMPTS - seat.attempts).max(0) as u32,
	}
}

#[async_trait]
impl ConsiliumRepository for PgConsilia {
	async fn owner_roster(&self) -> Result<Vec<UserId>, DomainError> {
		// The DENOMINATOR counts every seat the identity plane has granted, frozen ones
		// included: a larger N raises the threshold, and only roster movements that raise
		// the bar are safe to honour. Countability at tally time is stricter — see
		// `seat_columns!`. Ordered so the snapshot is reproducible.
		let ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM users WHERE role = 'owner' ORDER BY id")
			.fetch_all(&self.pool)
			.await
			.map_err(repo_err)?;
		Ok(ids.into_iter().map(UserId::from_raw).collect())
	}

	async fn open(&self, consilium: &mut Consilium, credentials: &[VoterCredential], approval_url_base: &str) -> Result<(), DomainError> {
		let mut tx = self.pool.begin().await.map_err(repo_err)?;
		let initiator_email = email_of(&mut tx, consilium.initiator()).await?;
		let terms = serde_json::to_string(&StoredTerms::of(consilium.terms())).map_err(|e| DomainError::Repository(e.to_string()))?;
		let inserted = sqlx::query(
			"INSERT INTO consilium (id, kind, state, terms, payload_hash, initiator_user_id, owner_count, threshold, expires_at, version) \
			 VALUES ($1, $2, $3, $4::jsonb, $5, $6, $7, $8, to_timestamp($9), $10)",
		)
		.bind(consilium.id().raw())
		.bind(consilium.kind().as_str())
		.bind(consilium.state().as_str())
		.bind(terms)
		.bind(consilium.payload_hash().as_slice())
		.bind(consilium.initiator().raw())
		.bind(consilium.owner_count() as i32)
		.bind(consilium.threshold() as i32)
		.bind(consilium.expires_at() as f64)
		.bind(consilium.version() as i64)
		.execute(&mut *tx)
		.await;
		if let Err(sqlx::Error::Database(err)) = &inserted
			&& err.code().as_deref() == Some(UNIQUE_VIOLATION)
		{
			// The partial unique index spoke. One consilium at a time is the whole of the
			// concurrent-approval overdraw defence, so this is a refusal, not a retry.
			return Err(DomainError::Conflict("a consilium is already open — cancel it before opening another".into()));
		}
		inserted.map_err(repo_err)?;

		for credential in credentials {
			sqlx::query(
				"INSERT INTO consilium_voter (consilium_id, user_id, initiator_user_id, token_hash, code_hash, expires_at) \
				 VALUES ($1, $2, $3, $4, $5, to_timestamp($6))",
			)
			.bind(consilium.id().raw())
			.bind(credential.user_id.raw())
			.bind(consilium.initiator().raw())
			.bind(credential.token_hash.as_slice())
			.bind(credential.code_hash.as_slice())
			.bind(consilium.expires_at() as f64)
			.execute(&mut *tx)
			.await
			.map_err(repo_err)?;

			let mail = GovernanceMail::PayoutApproval(PayoutApproval {
				consilium_id: consilium.id().to_string(),
				initiator_email: initiator_email.clone(),
				network: consilium.terms().network.as_str().to_owned(),
				address: consilium.terms().address.as_str().to_owned(),
				amount: consilium.terms().amount.to_decimal_string(),
				memo: consilium.terms().memo.clone(),
				payload_hash: consilium.payload_hash_hex(),
				threshold: consilium.threshold(),
				owner_count: consilium.owner_count(),
				expires_at: consilium.expires_at(),
				approval_url: format!("{}/{}", approval_url_base.trim_end_matches('/'), credential.token),
				code: credential.code.clone(),
			});
			let key = format!("consilium:{}:approval:{}", consilium.id(), credential.user_id);
			enqueue(&mut tx, consilium.id().raw(), credential.user_id.raw(), &key, &mail).await?;
		}

		outbox::drain_to_outbox(&mut tx, consilium, false).await?;
		tx.commit().await.map_err(repo_err)?;
		Ok(())
	}

	async fn find(&self, id: ConsiliumId) -> Result<Option<ConsiliumView>, DomainError> {
		let mut conn = self.pool.acquire().await.map_err(repo_err)?;
		let Some(row) = sqlx::query(concat!("SELECT ", consilium_columns!(), " FROM consilium c WHERE c.id = $1"))
			.bind(id.raw())
			.fetch_optional(&mut *conn)
			.await
			.map_err(repo_err)?
		else {
			return Ok(None);
		};
		let seats = seats_of(&mut conn, id.raw()).await?;
		let consilium = rehydrate(&row, &seats)?;
		let initiator_email = email_of(&mut conn, consilium.initiator()).await?;
		Ok(Some(view_of(consilium, initiator_email, &seats)?))
	}

	/// THREE QUERIES, NOT `2N + 1`.
	///
	/// This used to run a seats query and an initiator-email query per row — 401 round trips
	/// to render 200 consilia, every one of them a network hop an operator waits through.
	/// The seats and the initiator addresses are fetched once each with `= ANY($1)` and
	/// grouped in memory; the page is small and bounded by `limit`, so holding it is free.
	async fn list(&self, limit: i64) -> Result<Vec<ConsiliumView>, DomainError> {
		let mut conn = self.pool.acquire().await.map_err(repo_err)?;
		let rows = sqlx::query(concat!("SELECT ", consilium_columns!(), " FROM consilium c ORDER BY c.created_at DESC LIMIT $1"))
			.bind(limit)
			.fetch_all(&mut *conn)
			.await
			.map_err(repo_err)?;
		if rows.is_empty() {
			return Ok(Vec::new());
		}
		let ids = rows.iter().map(|row| row.try_get::<Uuid, _>("id")).collect::<Result<Vec<_>, _>>().map_err(repo_err)?;
		let initiators = rows
			.iter()
			.map(|row| row.try_get::<Uuid, _>("initiator_user_id"))
			.collect::<Result<Vec<_>, _>>()
			.map_err(repo_err)?;

		let mut seats_by_consilium: HashMap<Uuid, Vec<SeatRow>> = HashMap::new();
		for row in &sqlx::query(seats_of_many_query!()).bind(&ids).fetch_all(&mut *conn).await.map_err(repo_err)? {
			let consilium_id: Uuid = row.try_get("consilium_id").map_err(repo_err)?;
			seats_by_consilium.entry(consilium_id).or_default().push(seat_of(row)?);
		}

		let mut email_by_user: HashMap<Uuid, String> = HashMap::new();
		for row in &sqlx::query("SELECT id, email FROM users WHERE id = ANY($1)")
			.bind(&initiators)
			.fetch_all(&mut *conn)
			.await
			.map_err(repo_err)?
		{
			email_by_user.insert(row.try_get("id").map_err(repo_err)?, row.try_get("email").map_err(repo_err)?);
		}

		let mut views = Vec::with_capacity(rows.len());
		for row in &rows {
			let id: Uuid = row.try_get("id").map_err(repo_err)?;
			// A consilium always has at least one seat (N >= 3 with the initiator barred),
			// but an empty slice rehydrates to a tally of zero rather than panicking.
			let seats = seats_by_consilium.remove(&id).unwrap_or_default();
			let consilium = rehydrate(row, &seats)?;
			let initiator_email = email_by_user
				.get(&consilium.initiator().raw())
				.cloned()
				.ok_or_else(|| DomainError::Repository(format!("consilium initiator {} has no mirrored user row", consilium.initiator())))?;
			views.push(view_of(consilium, initiator_email, &seats)?);
		}
		Ok(views)
	}

	async fn cancel(&self, id: ConsiliumId, by: UserId, at: i64) -> Result<ConsiliumView, DomainError> {
		let mut tx = self.pool.begin().await.map_err(repo_err)?;
		let (mut consilium, seats) = locked(&mut tx, id).await?;
		if consilium.initiator() != by {
			return Err(DomainError::Forbidden("only the owner who opened this consilium may withdraw it".into()));
		}
		consilium.cancel(at)?;
		persist(&mut tx, &mut consilium).await?;
		announce(&mut tx, &consilium, "withdrawn by the owner who opened it").await?;
		let initiator_email = email_of(&mut tx, consilium.initiator()).await?;
		tx.commit().await.map_err(repo_err)?;
		view_of(consilium, initiator_email, &seats)
	}

	async fn invitation(&self, token_hash: &[u8; DIGEST_BYTES], at: i64) -> Result<InvitationView, DomainError> {
		// STRICTLY read-only. Gmail, Outlook SafeLinks and corporate gateways fetch every
		// URL in a message; were this to count an attempt or spend the token, a scanner
		// would burn an owner's vote before they ever opened the mail.
		let mut conn = self.pool.acquire().await.map_err(repo_err)?;
		let Some(seat) = sqlx::query(token_query!())
			.bind(token_hash.as_slice())
			.fetch_optional(&mut *conn)
			.await
			.map_err(repo_err)?
			.map(token_seat_of)
			.transpose()?
		else {
			return Err(invitation_not_found());
		};
		let seats = seats_of(&mut conn, seat.consilium_id).await?;
		let consilium = rehydrate(&seat.consilium, &seats)?;
		// Unknown, expired, burned, spent and closed all leave by the same door, so this
		// surface cannot be used to work out which one a given token hit.
		if seat.burned || seat.used || seat.token_expires_at <= at || !consilium.state().is_open() {
			return Err(invitation_not_found());
		}
		let initiator_email = email_of(&mut conn, consilium.initiator()).await?;
		Ok(invitation_of(&consilium, initiator_email, &seat, VoteDecision::Pending))
	}

	async fn submit(&self, token_hash: &[u8; DIGEST_BYTES], code: &str, decision: VoteDecision, audit: &VoteAudit, at: i64) -> Result<SubmitOutcome, DomainError> {
		let mut tx = self.pool.begin().await.map_err(repo_err)?;
		// LOCK ORDER: THE CONSILIUM FIRST, ALWAYS.
		//
		// Every other transition here goes through `locked()`, which takes the consilium row
		// and only then its seats. A single `... FOR UPDATE OF c, v` cannot promise that
		// order — Postgres locks rows in the order it reaches them, and this statement
		// drives from `consilium_voter`, so the seat tends to be locked first. Two owners
		// voting at the same moment would then take the two locks in opposite orders and
		// deadlock, turning one perfectly valid vote into a 503.
		//
		// So: resolve the token with an UNLOCKED read to learn which consilium it belongs
		// to, take the consilium lock through the same `locked()` every sibling uses, and
		// only then re-read the seat `FOR UPDATE`. The re-read is what makes the unlocked
		// first hop safe — every value the decision below depends on (attempts, burned,
		// used, decision) is taken under the lock, not from the probe.
		let Some(probe) = sqlx::query_scalar::<_, Uuid>("SELECT consilium_id FROM consilium_voter WHERE token_hash = $1")
			.bind(token_hash.as_slice())
			.fetch_optional(&mut *tx)
			.await
			.map_err(repo_err)?
		else {
			return Err(invitation_not_found());
		};
		let (mut consilium, _seats) = locked(&mut tx, ConsiliumId::from_raw(probe)).await?;
		let Some(mut seat) = sqlx::query(concat!(token_query!(), " FOR UPDATE OF v"))
			.bind(token_hash.as_slice())
			.fetch_optional(&mut *tx)
			.await
			.map_err(repo_err)?
			.map(token_seat_of)
			.transpose()?
		else {
			return Err(invitation_not_found());
		};
		let voter = UserId::from_raw(seat.user_id);
		let already = VoteDecision::parse(&seat.decision)?;
		let initiator_email = email_of(&mut tx, consilium.initiator()).await?;

		// A burned token and an expired one are indistinguishable from an unknown one. The
		// token's OWN deadline is checked here rather than trusting the sweep to have run,
		// which is what makes "an expired consilium can never execute" hold even when a
		// vote arrives late on one the sweeper has not reached yet.
		if seat.burned || seat.token_expires_at <= at {
			return Err(invitation_not_found());
		}
		// A closed consilium accepts nothing new. A seat that already answered still gets
		// through, so a retried submission can resolve to its idempotent answer below.
		if !consilium.state().is_open() && already == VoteDecision::Pending {
			return Err(invitation_not_found());
		}

		let correct = ct_eq(&digest(code.as_bytes()), &seat.code_hash);

		// ONLY A SEAT THAT CAN STILL CHANGE SOMETHING PAYS FOR A GUESS.
		//
		// The increment used to be unconditional, which broke two ways at once. A seat that
		// answered correctly on its fifth attempt sits at exactly 5 without being burned
		// (the correct-code path never burns); the next POST — a double-click, a mail
		// gateway retrying — pushed it to 6, violated `CHECK (attempts <= 5)`, and turned
		// that seat's every future request into a 503, destroying the idempotent-retry
		// contract. The same ordering also meant a seat already at 5 aborted on the CHECK
		// before it could reach the burn below.
		//
		// A seat that has already decided is charged nothing: its vote is recorded and
		// immutable (a repeat of the same answer is a no-op, a different one is refused), so
		// there is nothing left for an attempt to protect. `LEAST` clamps the counted path
		// as defence in depth, and `RETURNING` keeps the Rust value and the column identical
		// rather than mirroring the arithmetic in two places.
		if already == VoteDecision::Pending {
			// Counted BEFORE comparing, in this same transaction: comparing first and
			// counting after would let N concurrent guesses all read the same pre-increment
			// counter and all get a free try.
			seat.attempts = sqlx::query_scalar("UPDATE consilium_voter SET attempts = LEAST(attempts + 1, $3) WHERE consilium_id = $1 AND user_id = $2 RETURNING attempts")
				.bind(seat.consilium_id)
				.bind(seat.user_id)
				.bind(MAX_CODE_ATTEMPTS)
				.fetch_one(&mut *tx)
				.await
				.map_err(repo_err)?;

			if !correct {
				if seat.attempts >= MAX_CODE_ATTEMPTS {
					sqlx::query("UPDATE consilium_voter SET burned_at = now() WHERE consilium_id = $1 AND user_id = $2")
						.bind(seat.consilium_id)
						.bind(seat.user_id)
						.execute(&mut *tx)
						.await
						.map_err(repo_err)?;
					announce_burn(&mut tx, &consilium, voter).await?;
					tx.commit().await.map_err(repo_err)?;
					// From here on this token answers exactly like an unknown one.
					return Err(invitation_not_found());
				}
				// The attempt must survive the refusal, so this commits rather than rolling back.
				tx.commit().await.map_err(repo_err)?;
				let remaining = MAX_CODE_ATTEMPTS - seat.attempts;
				return Err(wrong_code(format!("incorrect code — {remaining} attempts remaining")));
			}
		} else if !correct {
			// An answered seat, wrong code: nothing was counted, so nothing can burn.
			return Err(wrong_code("incorrect code".to_owned()));
		}

		let before = consilium.version();
		consilium.record_vote(voter, decision, at)?;
		if consilium.version() == before {
			// The same answer as before: an idempotent no-op, so nothing but the attempt is
			// written. A retried request must not error, and must not double-count.
			tx.commit().await.map_err(repo_err)?;
			return Ok(SubmitOutcome {
				invitation: invitation_of(&consilium, initiator_email, &seat, already),
				decided: false,
				approved: false,
			});
		}

		sqlx::query(
			// `payload_hash` is captured per seat, not just per consilium: `execute` already
			// re-verifies the hash over the whole request, but nothing recorded WHICH terms each
			// owner personally signed. With it an auditor can prove, seat by seat, that the terms
			// shown at the moment of that signature are the terms that were paid.
			"UPDATE consilium_voter SET decision = $3, decided_at = to_timestamp($4), used_at = to_timestamp($4), client_ip = $5, user_agent = $6, payload_hash = $7 \
			 WHERE consilium_id = $1 AND user_id = $2",
		)
		.bind(seat.consilium_id)
		.bind(seat.user_id)
		.bind(decision.as_str())
		.bind(at as f64)
		.bind(&audit.client_ip)
		.bind(&audit.user_agent)
		.bind(consilium.payload_hash().as_slice())
		.execute(&mut *tx)
		.await
		.map_err(repo_err)?;

		let decided = !consilium.state().is_open();
		let approved = consilium.state() == ConsiliumState::Approved;
		persist(&mut tx, &mut consilium).await?;
		// An approval is announced by the execution step instead, once there is an outcome
		// worth reading; announcing both would mail the owners twice about one event.
		if decided && !approved {
			announce(&mut tx, &consilium, "the threshold can no longer be reached").await?;
		}
		tx.commit().await.map_err(repo_err)?;
		Ok(SubmitOutcome {
			invitation: invitation_of(&consilium, initiator_email, &seat, decision),
			decided,
			approved,
		})
	}

	/// ONE CLOCK, AND NO ROW CAN KILL THE SWEEP.
	///
	/// The selection used to read Postgres `now()` while the aggregate was expired against
	/// the caller's `at`. Two clocks: whenever the app's lagged even slightly behind the
	/// database's, a row came back due and `Consilium::expire` then refused it as not yet
	/// due — and the `?` on that refusal aborted the WHOLE sweep, so every later consilium
	/// in the batch silently stayed open. Binding `at` for the selection too means the row
	/// set and the transition are answering the same question.
	///
	/// Per-row failures now warn and continue for the same reason: one wedged consilium must
	/// not keep the rest of the queue from closing.
	async fn expire_due(&self, at: i64) -> Result<usize, DomainError> {
		let due: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM consilium WHERE state = 'open' AND expires_at <= to_timestamp($1)")
			.bind(at)
			.fetch_all(&self.pool)
			.await
			.map_err(repo_err)?;
		let mut closed = 0;
		for id in due {
			match self.expire_one(id, at).await {
				Ok(true) => closed += 1,
				Ok(false) => {}
				Err(err) => tracing::warn!(consilium_id = %id, "consilium: could not expire (will retry next sweep): {err}"),
			}
		}
		Ok(closed)
	}

	async fn last_roster_change_at(&self) -> Result<Option<i64>, DomainError> {
		sqlx::query_scalar::<_, Option<i64>>("SELECT EXTRACT(EPOCH FROM MAX(changed_at))::bigint FROM governance_roster_change")
			.fetch_one(&self.pool)
			.await
			.map_err(repo_err)
	}

	async fn void_open_for_roster_change(&self, changed_at: i64, at: i64) -> Result<usize, DomainError> {
		// Only a consilium that was ALREADY open when the roster moved is voided. One opened
		// afterwards is either inside the cooling-off window (and so was refused at open) or
		// legitimately past it, and must not be cancelled by a change it already knows about.
		// `+ 1` compensates for `last_roster_change_at` truncating to whole seconds: a change
		// recorded at T.9 reads back as T, and a consilium created at T.5 was genuinely open
		// when it landed. The ambiguous same-second case is resolved toward VOIDING, which is
		// the fail-safe direction — a needlessly voided request can simply be proposed again.
		let stale: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM consilium WHERE state = 'open' AND created_at < to_timestamp($1 + 1)")
			.bind(changed_at)
			.fetch_all(&self.pool)
			.await
			.map_err(repo_err)?;
		let mut voided = 0;
		for id in stale {
			match self.void_one(id, at).await {
				Ok(true) => voided += 1,
				Ok(false) => {}
				Err(err) => tracing::warn!(consilium_id = %id, "consilium: could not void after a roster change (will retry next sweep): {err}"),
			}
		}
		Ok(voided)
	}

	async fn awaiting_execution(&self) -> Result<Vec<ConsiliumId>, DomainError> {
		// `execution_failed` is deliberately absent: nothing retries silently.
		let ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM consilium WHERE state = 'approved' ORDER BY created_at")
			.fetch_all(&self.pool)
			.await
			.map_err(repo_err)?;
		Ok(ids.into_iter().map(ConsiliumId::from_raw).collect())
	}

	async fn record_execution(&self, id: ConsiliumId, outcome: ExecutionOutcome, at: i64) -> Result<ConsiliumView, DomainError> {
		let mut tx = self.pool.begin().await.map_err(repo_err)?;
		let (mut consilium, seats) = locked(&mut tx, id).await?;
		let detail = match outcome {
			ExecutionOutcome::Executed(withdrawal) => {
				consilium.mark_executed(withdrawal, at)?;
				format!("payout {withdrawal} created")
			}
			ExecutionOutcome::Failed(reason) => {
				consilium.mark_execution_failed(reason.clone(), at)?;
				reason
			}
		};
		persist(&mut tx, &mut consilium).await?;
		announce(&mut tx, &consilium, &detail).await?;
		let initiator_email = email_of(&mut tx, consilium.initiator()).await?;
		tx.commit().await.map_err(repo_err)?;
		view_of(consilium, initiator_email, &seats)
	}
}
