//! Outbox relay — the saga dispatcher.
//!
//! A single worker drains undispatched [`outbox`] rows in strict
//! `seq` order and applies each to TigerBeetle (and, for withdrawals, the custody
//! service) **after** the control-plane commit (Write-Last). "Single worker" is an
//! enforced invariant, not a deploy assumption: [`Relay::run`] holds a fixed-key session
//! `pg_advisory_lock` and only drains while held, so a second core instance blocks on
//! the lock and never touches the outbox (see `OUTBOX_LOCK_KEY`); losing the lock's
//! connection re-enters that acquisition (with backoff) rather than exiting, so a DB
//! blip never cascades into a whole-process teardown.
//! Delivery is
//! at-least-once, so every op is idempotent: ledger transfer ids are deterministic
//! (a posted transfer uses the event id; a reservation's pending uses an
//! aggregate-derived id so its completion can reference it), the gateway treats
//! `Exists`/`AlreadyPosted`/`AlreadyVoided` as success, and the custody broadcast is
//! idempotent by withdrawal id.
//!
//! Strict order means a reservation's pending is always applied before its
//! settle/cancel, so a two-phase completion never races its own pending. Transient
//! failures (`Unavailable`/`Retryable`, or a custody outage) stop the cycle and retry
//! from the same `seq`; a genuine inconsistency, `InsufficientFunds`, or a custody
//! rejection is **parked** — moved to a distinct `parked_at` terminal state (NOT
//! `dispatched_at`) so the row stays queryable yet is excluded from the drain, with a
//! loud (Sentry-shipped) `error!` + `last_error`. One bad event can't wedge the queue; a
//! park *after* an earlier leg posted is flagged half-applied (`compensated_at`). The
//! [`super::reconciliation`] job and the [`super::reaper`] then own recovery: nothing is
//! silently dropped.

use std::{sync::Arc, time::Duration};

use domain::{
	balance::{LedgerAccountKey, LedgerEvent, TransferCode},
	money::Usdt,
	redemptions::RedemptionEvent,
	subscriptions::SubscriptionEvent,
	users::UserId,
	withdrawals::WithdrawalEvent,
};
use sqlx::{PgPool, pool::PoolConnection, postgres::Postgres};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{
	infrastructure::outbox::{self, OutboxRow},
	ports::{
		custody::{BroadcastRequest, Custody, CustodyError},
		ledger::{CompletionKind, Ledger, LedgerError, LedgerTransfer, PendingCompletion},
	},
};

/// Distinct salts deriving a withdrawal's deterministic TigerBeetle transfer ids from
/// the (stable) withdrawal id. The reservation locks the gross against the user's claim
/// into clearing; settle posts that pending and redistributes the net to the rail's
/// custody and the fee to fee-revenue; fail/cancel void the pending (refund). A
/// completion references the reservation's id as its `pending_id`.
const CLEARING_RESERVE: &[u8] = b"withdraw:clearing";
const CLEARING_SETTLE: &[u8] = b"withdraw:clearing:settle";
const WALLET_REDISTRIBUTE: &[u8] = b"withdraw:wallet";
const FEE_REDISTRIBUTE: &[u8] = b"withdraw:fee";
const CLEARING_VOID_FAIL: &[u8] = b"withdraw:clearing:void:fail";
const CLEARING_VOID_CANCEL: &[u8] = b"withdraw:clearing:void:cancel";

/// Salts for a subscription's two posted legs (one event → two transfers, so they can't
/// share the event id): the cash move `Dr user / Cr service` and the unit mint
/// `Dr user-shares / Cr shares-outstanding`. Cash-leg first, so an insufficient claim
/// parks before any units are minted.
const SUBSCRIBE_CASH: &[u8] = b"subscribe:cash";
const SUBSCRIBE_MINT: &[u8] = b"subscribe:mint";

/// Salts for a redemption's saga: the reservation locks the units as a pending burn;
/// settle posts the burn and pays the cash out of the fund's claim; fail/cancel void the
/// burn (returning the units). A completion references the reservation's id as `pending_id`.
const BURN_RESERVE: &[u8] = b"redeem:burn";
const BURN_SETTLE: &[u8] = b"redeem:burn:settle";
const REDEEM_PAYOUT: &[u8] = b"redeem:payout";
const BURN_VOID_FAIL: &[u8] = b"redeem:burn:void:fail";
const BURN_VOID_CANCEL: &[u8] = b"redeem:burn:void:cancel";

/// Bound on consecutive `RetryBounded` attempts before a never-resolving retryable (a
/// completion whose pending was itself parked, so it can never be found) is parked
/// rather than wedging the single-worker queue forever.
const MAX_RETRYABLE_ATTEMPTS: i32 = 25;

/// Fixed key for the session-level `pg_advisory_lock` the relay holds while draining.
/// The whole ordering/atomicity argument (strict `seq`, reserve-before-complete, the
/// settle-time liquidity pre-check) is valid *only* with a single drainer; this lock
/// makes that singleton an enforced invariant rather than a deploy assumption — a
/// second instance blocks here and never touches the outbox. The value is an arbitrary
/// stable constant (ASCII "EVBKOBX_"); only stability matters — changing it would let
/// two cohorts drain concurrently across a deploy that mixed old and new keys.
const OUTBOX_LOCK_KEY: i64 = 0x4556_424b_4f42_585f_u64 as i64;

/// Salt deriving the subscription projection marker's id, distinct from any TB transfer salt
/// so the `saga_steps.tb_transfer_id` unique constraint never aliases a real transfer.
const SUBSCRIBE_POSITION: &[u8] = b"subscribe:position";
/// The relay task: drains the outbox to the ledger + custody. Cloneable handles
/// (`pool`, `ledger`, `custody`, `notify`) so command handlers can `notify` it to
/// dispatch promptly.
pub struct Relay {
	pool: PgPool,
	ledger: Arc<dyn Ledger>,
	custody: Arc<dyn Custody>,
	notify: Arc<Notify>,
}

impl Relay {
	pub fn new(pool: PgPool, ledger: Arc<dyn Ledger>, custody: Arc<dyn Custody>, notify: Arc<Notify>) -> Self {
		Self { pool, ledger, custody, notify }
	}

	/// Run until `shutdown` is cancelled. Take the singleton outbox lock on a dedicated
	/// connection (blocking until acquired — a second instance idles here, owning
	/// nothing), then drain the backlog and wait for a nudge or a poll-fallback. The held
	/// connection keeps the session-level lock for as long as it lives; dropping it
	/// (process exit, or a lost backend) releases it so a standby can take over.
	///
	/// A transient DB failure — the lock acquisition erroring, or the lock-holding
	/// connection dropping mid-run — must NOT return: the composition root cancels every
	/// sibling task when any branch completes, so an early return turns a DB blip into a
	/// full money-plane teardown. Instead the outer loop re-acquires the lock with capped
	/// exponential backoff (a standby may legitimately win it first — we then block on it,
	/// which is exactly the singleton hand-off working).
	///
	/// Graceful shutdown is cooperative: cancellation is only observed at the wait points
	/// *between* drains and during the lock/backoff waits, never mid-`drain`. Each `drain`
	/// iteration runs to completion before exit (it is already crash-safe to stop between
	/// rows), so an in-flight dispatch is never torn down partway — a deploy/restart leaves
	/// the outbox in the same clean state a graceful drain does.
	pub async fn run(self, shutdown: CancellationToken) {
		const MAX_BACKOFF: Duration = Duration::from_secs(30);
		let mut backoff = Duration::from_millis(500);
		'acquire: loop {
			let mut lock = tokio::select! {
				biased;
				() = shutdown.cancelled() => return,
				lock = self.acquire_outbox_lock() => match lock {
					Ok(lock) => lock,
					Err(err) => {
						error!("relay: could not acquire the outbox lock (retrying in {backoff:?}): {err}");
						tokio::select! {
							() = shutdown.cancelled() => return,
							() = tokio::time::sleep(backoff) => {},
						}
						backoff = (backoff * 2).min(MAX_BACKOFF);
						continue 'acquire;
					}
				},
			};
			backoff = Duration::from_millis(500);
			info!("relay: acquired the outbox lock — draining as the singleton worker");
			loop {
				let throttle = self.drain().await;
				if shutdown.is_cancelled() {
					info!("relay: shutdown requested — drain iteration complete, stopping");
					return;
				}
				let wait = if throttle { Duration::from_secs(2) } else { Duration::from_millis(500) };
				tokio::select! {
					() = shutdown.cancelled() => {
						info!("relay: shutdown requested — stopping");
						return;
					},
					() = self.notify.notified() => {},
					() = tokio::time::sleep(wait) => {},
				}
				// Touch the lock-holding connection so a dropped backend surfaces here rather
				// than letting a lockless relay silently keep draining. Postgres released the
				// lock with the dead session, so re-acquiring (not exiting) is the recovery.
				if let Err(err) = sqlx::query("SELECT 1").execute(lock.as_mut()).await {
					error!("relay: lost the outbox lock connection — re-acquiring: {err}");
					continue 'acquire;
				}
			}
		}
	}

	/// Block on a dedicated pooled connection until the fixed-key session advisory lock
	/// is held, then return that connection — its lifetime *is* the lock's. The first
	/// instance returns immediately; any other blocks inside Postgres until the holder's
	/// connection closes.
	pub async fn acquire_outbox_lock(&self) -> Result<PoolConnection<Postgres>, sqlx::Error> {
		let mut conn = self.pool.acquire().await?;
		sqlx::query("SELECT pg_advisory_lock($1)").bind(OUTBOX_LOCK_KEY).execute(conn.as_mut()).await?;
		Ok(conn)
	}

	/// Non-blocking sibling of [`acquire_outbox_lock`](Self::acquire_outbox_lock): take
	/// the lock if free (returning the holding connection), else `None` — used by tests
	/// to observe that a held lock makes a second drainer back off without blocking.
	pub async fn try_acquire_outbox_lock(&self) -> Result<Option<PoolConnection<Postgres>>, sqlx::Error> {
		let mut conn = self.pool.acquire().await?;
		let acquired: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)").bind(OUTBOX_LOCK_KEY).fetch_one(conn.as_mut()).await?;
		Ok(acquired.then_some(conn))
	}

	/// Drain the current backlog in `seq` order. Returns `true` if it stopped on a
	/// transient failure (caller should back off before retrying). Public so
	/// integration tests can apply committed events deterministically (one call
	/// processes the whole backlog).
	pub async fn drain(&self) -> bool {
		loop {
			let batch = match outbox::next_batch(&self.pool, 128).await {
				Ok(batch) => batch,
				Err(err) => {
					warn!("relay: reading the outbox failed: {err}");
					return true;
				}
			};
			if batch.is_empty() {
				return false;
			}
			let mut advanced = false;
			for row in &batch {
				match self.dispatch(row).await {
					Outcome::Done => {
						if let Err(err) = outbox::mark_dispatched(&self.pool, row.seq).await {
							warn!(seq = row.seq, "relay: failed to mark dispatched (event will re-deliver): {err}");
						}
						advanced = true;
					}
					Outcome::Park { reason, applied_legs } => {
						self.park(row, &reason, applied_legs).await;
						advanced = true;
					}
					Outcome::Retry(reason) => {
						warn!(seq = row.seq, "relay: transient failure (unbounded), retrying from here: {reason}");
						if let Err(err) = outbox::record_failure(&self.pool, row.seq, &reason).await {
							warn!(seq = row.seq, "relay: failed to record the retry attempt: {err}");
						}
						return true;
					}
					Outcome::RetryBounded(reason) => {
						// A retryable that can never resolve (a completion whose pending was
						// itself parked, so it is never found) must not wedge the single-worker
						// queue forever — park it after a bound so later events keep flowing.
						if row.attempts + 1 >= MAX_RETRYABLE_ATTEMPTS {
							self.park(row, &format!("retryable exhausted after {} attempts: {reason}", row.attempts + 1), 0).await;
							advanced = true;
						} else {
							if let Err(err) = outbox::record_failure(&self.pool, row.seq, &reason).await {
								warn!(seq = row.seq, "relay: failed to record the retry attempt: {err}");
							}
							warn!(seq = row.seq, attempts = row.attempts + 1, "relay: retryable, retrying from here: {reason}");
							return true;
						}
					}
				}
			}
			if !advanced {
				return false;
			}
		}
	}

	/// Park a non-retryable event into its distinct terminal state (NOT dispatched), so it
	/// stays queryable for reconciliation/the reaper instead of being silently dropped. If
	/// any earlier leg of a multi-leg event already applied (`applied_legs > 0`), the event
	/// is half-applied: stamp a compensation marker and alert at `error!` (which the Sentry
	/// `tracing_layer` ships) so the aggregate's PG-vs-TB divergence is surfaced. Auto-
	/// reversing the applied TB legs stays a follow-up — reconciliation owns recovery today.
	async fn park(&self, row: &OutboxRow, reason: &str, applied_legs: usize) {
		if let Err(err) = outbox::mark_parked(&self.pool, row.seq, reason).await {
			error!(seq = row.seq, "relay: failed to stamp parked_at (event will re-deliver): {err}");
		}
		if applied_legs > 0 {
			error!(seq = row.seq, event_id = %row.event_id, aggregate = %row.aggregate, applied_legs, "relay: PARKED HALF-APPLIED event (compensation owed): {reason}");
			if let Err(err) = outbox::mark_compensated(&self.pool, row.seq).await {
				error!(seq = row.seq, "relay: failed to stamp compensated_at: {err}");
			}
		} else {
			error!(seq = row.seq, event_id = %row.event_id, aggregate = %row.aggregate, "relay: parking event (needs intervention): {reason}");
		}
	}

	async fn dispatch(&self, row: &OutboxRow) -> Outcome {
		let ops = match plan(row) {
			Ok(ops) => ops,
			Err(reason) => return Outcome::park(format!("unplannable event: {reason}")),
		};
		// Settle-time liquidity pre-check. The relay is single-worker and sequential, so a
		// Read-First check over *all* legs here guarantees a gated leg cannot fail with
		// InsufficientFunds mid-event (which would half-apply and strand value). A shortfall
		// parks the whole event atomically (nothing applied); a top-up + reconciliation
		// recovers it. Two gated legs, on opposite sides:
		//   - `withdraw_disburse` — the net leaves a rail's custody (`Cr wallet:<net>`), so
		//     guard that *credit* account's posted liquidity.
		//   - `redeem_payout` — cash leaves the fund's claim (`Dr service`), so guard that
		//     *debit* account's available balance (other queued payouts may hold it).
		for op in &ops {
			let LedgerAction::Post(transfer) = &op.action else { continue };
			let (guarded, liquidity) = match op.role {
				"withdraw_disburse" => (&transfer.credit, None),
				"redeem_payout" => (&transfer.debit, Some(())),
				_ => continue,
			};
			match self.ledger.balance(guarded).await {
				Ok(balance) => {
					let have = if liquidity.is_some() { balance.available() } else { balance.posted };
					if have < transfer.amount {
						// Pre-check parks before any leg applies — nothing half-applied.
						return Outcome::park(format!("{} liquidity insufficient at settle", guarded.logical_key()));
					}
				}
				Err(LedgerError::Unavailable(err)) => return Outcome::Retry(err),
				Err(LedgerError::Retryable(err)) => return Outcome::RetryBounded(err),
				Err(err) => return Outcome::park(format!("settle liquidity check: {err}")),
			}
		}
		// The custody Broadcast is the one op with an external side effect and no TB leg
		// whose flags could refuse it, so it gets its own Read-First: the withdrawal's
		// clearing reservation must have **actually applied** — its deterministic transfer
		// id recorded in `saga_steps` by the (strictly earlier in `seq`) Requested leg.
		// Without this, a parked reserve (the optimistic solvency check reads TB, which
		// lags committed-but-undrained outbox rows, so a double-submit passes it twice)
		// leaves the trailing Dispatched row free to send real money on-chain with
		// nothing locked behind it — the over-withdrawal race.
		for op in &ops {
			if !matches!(op.action, LedgerAction::Broadcast(_)) {
				continue;
			}
			match reserve_applied(&self.pool, row.aggregate_id).await {
				Ok(true) => {}
				Ok(false) => return Outcome::park("withdrawal reserve never applied to the ledger (parked?) — refusing to broadcast".into()),
				Err(err) => return Outcome::Retry(format!("reserve-applied check: {err}")),
			}
			// The withdrawal row must still be `processing`: a `Dispatched` event unparked
			// AFTER the withdrawal was failed/cancelled (its reservation voided) would
			// otherwise send real money with nothing locked behind it — the runbook's
			// unpark-after-fail double-pay hazard, made structurally impossible here.
			match withdrawal_state(&self.pool, row.aggregate_id).await {
				Ok(Some(state)) if state == "processing" => {}
				Ok(Some(state)) => return Outcome::park(format!("withdrawal is {state}, not processing — refusing to broadcast")),
				Ok(None) => return Outcome::park("withdrawal row is missing — refusing to broadcast".into()),
				Err(err) => return Outcome::Retry(format!("broadcast-state check: {err}")),
			}
		}
		// Track applied legs so a park *after* an earlier leg posted (`applied > 0`) is flagged
		// half-applied — the genuine residual a balance pre-check can't predict (a bare TB
		// Conflict on a non-first leg). A first-leg park leaves nothing applied.
		let mut applied = 0usize;
		for (leg, op) in ops.iter().enumerate() {
			let result = match &op.action {
				LedgerAction::Post(transfer) => self.ledger.post(transfer).await,
				LedgerAction::Reserve(transfer) => self.ledger.reserve(transfer).await,
				LedgerAction::Complete(completion) => self.ledger.complete(completion).await,
				LedgerAction::Broadcast(request) => self.custody.broadcast(request).await.map_err(custody_to_ledger),
			};
			match result {
				Ok(()) => {
					// A broadcast is an external side effect, not a TB transfer — no saga step.
					// The step record is load-bearing (the broadcast guard above reads it), so
					// a failed insert retries the whole event: every leg is idempotent
					// (`Exists`/`AlreadyPosted` are success), so the redelivery re-applies
					// cleanly and records the step it owes.
					if !matches!(op.action, LedgerAction::Broadcast(_))
						&& let Err(err) = record_saga_step(&self.pool, row.event_id, leg as i32, op.role, op.transfer_id).await
					{
						return Outcome::Retry(format!("saga step record: {err}"));
					}
					applied += 1;
				}
				Err(LedgerError::Unavailable(err)) => return Outcome::Retry(err),
				Err(LedgerError::Retryable(err)) => return Outcome::RetryBounded(err),
				Err(LedgerError::InsufficientFunds) =>
					return Outcome::Park {
						reason: "insufficient funds".into(),
						applied_legs: applied,
					},
				Err(LedgerError::Conflict(err)) =>
					return Outcome::Park {
						reason: format!("ledger conflict: {err}"),
						applied_legs: applied,
					},
			}
		}
		// A subscription's cost-basis projection is written **here**, after both ledger legs
		// post — never on the synchronous open path, where a later parked cash-leg would leave
		// a phantom `fund_positions` row (basis without units or cash). The upsert is idempotent
		// under at-least-once delivery: a per-event `saga_steps` marker gates the relative add,
		// applied in the same transaction so a redelivery never double-counts the basis.
		if row.kind == "subscriptions"
			&& let Err(err) = project_subscription(&self.pool, row).await
		{
			return Outcome::Retry(format!("subscribe cost-basis projection: {err}"));
		}
		Outcome::Done
	}
}

/// Apply a settled subscription's cost-basis projection (`fund_positions.cost_basis +=
/// cash`, `units += minted units`, high-water mark = max) idempotently: a synthetic
/// `saga_steps` leg keys the apply to the event, so the relative add lands at most once even
/// on a redelivery. The upsert runs in the same transaction as that marker insert — both
/// commit or neither does. The projection-tracked `units` is the denominator the redemption
/// settle reduces the cost basis against (see [`super::redemptions::reduce_cost_basis`]),
/// kept off TB so it never lags the async burn.
async fn project_subscription(pool: &PgPool, row: &OutboxRow) -> Result<(), sqlx::Error> {
	const PROJECTION_LEG: i32 = 100;
	let SubscriptionEvent::Subscribed {
		user, service, cash, nav, units, ..
	} = serde_json::from_str(&row.payload).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
	let marker_id = tid(row.aggregate_id, SUBSCRIBE_POSITION);
	let mut tx = pool.begin().await?;
	let marked = sqlx::query("INSERT INTO saga_steps (event_id, leg, role, tb_transfer_id) VALUES ($1, $2, 'subscribe_position', $3) ON CONFLICT (event_id, leg) DO NOTHING")
		.bind(row.event_id)
		.bind(PROJECTION_LEG)
		.bind(&marker_id.to_be_bytes()[..])
		.execute(&mut *tx)
		.await?
		.rows_affected();
	if marked == 1 {
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
		.bind(cash.base_units().to_string())
		.bind(units.base_units().to_string())
		.bind(nav.base_units().to_string())
		.execute(&mut *tx)
		.await?;
	}
	tx.commit().await?;
	Ok(())
}

/// Custody failures fold into the existing ledger outcomes: an outage is transient
/// (retry — nothing was sent), a policy/liquidity refusal is parked.
fn custody_to_ledger(err: CustodyError) -> LedgerError {
	match err {
		CustodyError::Unavailable(detail) => LedgerError::Unavailable(format!("custody unavailable: {detail}")),
		CustodyError::Rejected(detail) => LedgerError::Conflict(format!("custody rejected: {detail}")),
	}
}

enum Outcome {
	Done,
	/// Transient infra outage (TB/custody unreachable) — stop and retry from this `seq`
	/// **unbounded** (it resolves when the dependency recovers).
	Retry(String),
	/// A retryable ledger state (a pending not yet visible) — retry from this `seq`, but
	/// **bounded**: a pending that can never appear (its reserve was itself parked) must
	/// not wedge the single-worker queue forever, so park after `MAX_RETRYABLE_ATTEMPTS`.
	RetryBounded(String),
	/// Terminal for this event — move it to the distinct **parked** state (stays queryable,
	/// excluded from the drain). `applied_legs` is how many legs of this (possibly
	/// multi-leg) event already posted before the park: `> 0` means it half-applied and a
	/// compensation is owed (reconciliation catches it; auto-reversal is a follow-up).
	Park {
		reason: String,
		applied_legs: usize,
	},
}

impl Outcome {
	/// Park before any leg applied — the common case (unplannable, pre-check shortfall).
	fn park(reason: String) -> Self {
		Outcome::Park { reason, applied_legs: 0 }
	}
}

struct PlannedOp {
	role: &'static str,
	transfer_id: u128,
	action: LedgerAction,
}

enum LedgerAction {
	Post(LedgerTransfer),
	Reserve(LedgerTransfer),
	Complete(PendingCompletion),
	Broadcast(BroadcastRequest),
}

/// Map an outbox event to the ledger op(s) (and any custody broadcast) it performs.
/// `reference` (stamped into `user_data_128`) is always the aggregate id, tying every
/// TB transfer back to its allocation/deposit/withdrawal for reconciliation.
fn plan(row: &OutboxRow) -> Result<Vec<PlannedOp>, String> {
	let reference = row.aggregate_id.as_u128();
	let event_tid = row.event_id.as_u128();
	match row.kind.as_str() {
		"balance" => {
			let event: LedgerEvent = serde_json::from_str(&row.payload).map_err(|e| e.to_string())?;
			Ok(vec![plan_balance(event, event_tid, reference)])
		}
		"withdrawals" => {
			let event: WithdrawalEvent = serde_json::from_str(&row.payload).map_err(|e| e.to_string())?;
			plan_withdrawal(event, row.aggregate_id, reference)
		}
		"subscriptions" => {
			let event: SubscriptionEvent = serde_json::from_str(&row.payload).map_err(|e| e.to_string())?;
			Ok(plan_subscription(event, row.aggregate_id, reference))
		}
		"redemptions" => {
			let event: RedemptionEvent = serde_json::from_str(&row.payload).map_err(|e| e.to_string())?;
			Ok(plan_redemption(event, row.aggregate_id, reference))
		}
		// A non-money event reached the outbox (shouldn't happen) — a benign no-op.
		_ => Ok(Vec::new()),
	}
}

fn plan_balance(event: LedgerEvent, event_tid: u128, reference: u128) -> PlannedOp {
	match event {
		LedgerEvent::Deposited { party, network, amount } => PlannedOp {
			role: "deposit",
			transfer_id: event_tid,
			action: LedgerAction::Post(LedgerTransfer {
				id: event_tid,
				debit: LedgerAccountKey::CryptoWallet(network),
				credit: party.claim_key(),
				amount: amount.base_units(),
				code: TransferCode::Deposit,
				reference,
			}),
		},
		LedgerEvent::CapitalSeeded { network, amount } => PlannedOp {
			role: "seed",
			transfer_id: event_tid,
			action: LedgerAction::Post(LedgerTransfer {
				id: event_tid,
				debit: LedgerAccountKey::CryptoWallet(network),
				credit: LedgerAccountKey::Fund,
				amount: amount.base_units(),
				code: TransferCode::SeedCapital,
				reference,
			}),
		},
	}
}

/// A subscription mints fund units in two posted legs, **cash-leg first**: move the cash
/// into the fund pool (`Dr user / Cr service`), then mint the units (`Dr user-shares /
/// Cr shares-outstanding`). The mint never fails (it only moves between two share
/// accounts whose supply we control), and an insufficient claim parks the cash leg before
/// the mint runs — so the relay can never strand minted units without the cash behind them.
fn plan_subscription(event: SubscriptionEvent, aggregate_id: Uuid, reference: u128) -> Vec<PlannedOp> {
	let SubscriptionEvent::Subscribed { user, service, cash, units, .. } = event;
	vec![
		PlannedOp {
			role: "subscribe_cash",
			transfer_id: tid(aggregate_id, SUBSCRIBE_CASH),
			action: LedgerAction::Post(LedgerTransfer {
				id: tid(aggregate_id, SUBSCRIBE_CASH),
				debit: LedgerAccountKey::UserClaim(user),
				credit: LedgerAccountKey::ServiceClaim(service.clone()),
				amount: cash.base_units(),
				code: TransferCode::Subscribe,
				reference,
			}),
		},
		PlannedOp {
			role: "subscribe_mint",
			transfer_id: tid(aggregate_id, SUBSCRIBE_MINT),
			action: LedgerAction::Post(LedgerTransfer {
				id: tid(aggregate_id, SUBSCRIBE_MINT),
				debit: LedgerAccountKey::UserShares(service.clone(), user),
				credit: LedgerAccountKey::SharesOutstanding(service),
				amount: units.base_units(),
				code: TransferCode::ShareMint,
				reference,
			}),
		},
	]
}

/// A redemption's saga in the ledger (accept-and-queue, settle-time priced):
/// - **Requested** → reserve a pending burn `Dr shares-outstanding / Cr user-shares`,
///   which locks the user's units (TigerBeetle's flag rejects an over-redeem here).
/// - **Settled** → **burn first, pay second**: post the pending burn, then pay the cash
///   `Dr service / Cr user`. The settle pre-check guards the payout's fund claim, so a
///   short fund parks before either leg; and burn-first means a parked reserve (a raced
///   over-redeem) fails the burn-post *before* any cash leaves — neither half-applies.
/// - **Failed/Cancelled** → void the pending burn (the units return to the user).
fn plan_redemption(event: RedemptionEvent, aggregate_id: Uuid, reference: u128) -> Vec<PlannedOp> {
	match event {
		RedemptionEvent::Requested { user, service, units, .. } => vec![PlannedOp {
			role: "redeem_reserve",
			transfer_id: tid(aggregate_id, BURN_RESERVE),
			action: LedgerAction::Reserve(LedgerTransfer {
				id: tid(aggregate_id, BURN_RESERVE),
				debit: LedgerAccountKey::SharesOutstanding(service.clone()),
				credit: LedgerAccountKey::UserShares(service, user),
				amount: units.base_units(),
				code: TransferCode::ShareBurn,
				reference,
			}),
		}],
		RedemptionEvent::Settled { user, service, units, cash, .. } => vec![
			PlannedOp {
				role: "redeem_burn",
				transfer_id: tid(aggregate_id, BURN_SETTLE),
				action: LedgerAction::Complete(PendingCompletion {
					id: tid(aggregate_id, BURN_SETTLE),
					pending_id: tid(aggregate_id, BURN_RESERVE),
					kind: CompletionKind::Post,
					debit: LedgerAccountKey::SharesOutstanding(service.clone()),
					credit: LedgerAccountKey::UserShares(service.clone(), user),
					amount: units.base_units(),
					code: TransferCode::ShareBurn,
					reference,
				}),
			},
			PlannedOp {
				role: "redeem_payout",
				transfer_id: tid(aggregate_id, REDEEM_PAYOUT),
				action: LedgerAction::Post(LedgerTransfer {
					id: tid(aggregate_id, REDEEM_PAYOUT),
					debit: LedgerAccountKey::ServiceClaim(service),
					credit: LedgerAccountKey::UserClaim(user),
					amount: cash.base_units(),
					code: TransferCode::Redeem,
					reference,
				}),
			},
		],
		RedemptionEvent::Failed { user, service, units, .. } => vec![void_burn(aggregate_id, user, service, units, reference, BURN_VOID_FAIL)],
		RedemptionEvent::Cancelled { user, service, units, .. } => vec![void_burn(aggregate_id, user, service, units, reference, BURN_VOID_CANCEL)],
	}
}

/// Void the pending burn (return the units) — shared by fail and cancel, which differ only
/// by the completion's deterministic id salt (a redemption reaches at most one).
fn void_burn(aggregate_id: Uuid, user: UserId, service: domain::balance::ServiceId, units: domain::money::Shares, reference: u128, salt: &[u8]) -> PlannedOp {
	PlannedOp {
		role: "redeem_void",
		transfer_id: tid(aggregate_id, salt),
		action: LedgerAction::Complete(PendingCompletion {
			id: tid(aggregate_id, salt),
			pending_id: tid(aggregate_id, BURN_RESERVE),
			kind: CompletionKind::Void,
			debit: LedgerAccountKey::SharesOutstanding(service.clone()),
			credit: LedgerAccountKey::UserShares(service, user),
			amount: units.base_units(),
			code: TransferCode::ShareBurn,
			reference,
		}),
	}
}

/// A withdrawal's saga in the ledger:
/// - **Requested** → reserve the gross as a pending `Dr user / Cr clearing` (no rail
///   touched, so acceptance never depends on rail liquidity).
/// - **Dispatched** → broadcast the net to custody (idempotent by withdrawal id).
/// - **Settled** → post the clearing pending, then move net→`wallet:<net>` and (when
///   non-zero) fee→`fee`. The `Cr wallet:<net>` is where rail liquidity is finally
///   checked by the non-negative flag.
/// - **Failed/Cancelled** → void the clearing pending, refunding the user in full.
fn plan_withdrawal(event: WithdrawalEvent, aggregate_id: Uuid, reference: u128) -> Result<Vec<PlannedOp>, String> {
	Ok(match event {
		WithdrawalEvent::Requested { user, amount, .. } => vec![PlannedOp {
			role: "withdraw_reserve",
			transfer_id: tid(aggregate_id, CLEARING_RESERVE),
			action: LedgerAction::Reserve(LedgerTransfer {
				id: tid(aggregate_id, CLEARING_RESERVE),
				debit: LedgerAccountKey::UserClaim(user),
				credit: LedgerAccountKey::WithdrawalClearing,
				amount: amount.base_units(),
				code: TransferCode::Withdraw,
				reference,
			}),
		}],
		WithdrawalEvent::Dispatched { network, address, amount, fee, .. } => {
			let net = amount.checked_sub(fee).ok_or("withdrawal fee exceeds amount")?;
			vec![PlannedOp {
				role: "withdraw_broadcast",
				transfer_id: 0,
				action: LedgerAction::Broadcast(BroadcastRequest {
					withdrawal_id: aggregate_id,
					network,
					address,
					amount: net,
				}),
			}]
		}
		WithdrawalEvent::Settled { user, network, amount, fee, .. } => {
			let net = amount.checked_sub(fee).ok_or("withdrawal fee exceeds amount")?;
			// Post the clearing reservation, then disburse: net leaves the rail's custody,
			// the fee is retained. The Vec order matters — the post must land before the
			// disbursements debit the now-posted clearing balance.
			let mut ops = vec![
				PlannedOp {
					role: "withdraw_settle",
					transfer_id: tid(aggregate_id, CLEARING_SETTLE),
					action: LedgerAction::Complete(PendingCompletion {
						id: tid(aggregate_id, CLEARING_SETTLE),
						pending_id: tid(aggregate_id, CLEARING_RESERVE),
						kind: CompletionKind::Post,
						debit: LedgerAccountKey::UserClaim(user),
						credit: LedgerAccountKey::WithdrawalClearing,
						amount: amount.base_units(),
						code: TransferCode::Withdraw,
						reference,
					}),
				},
				PlannedOp {
					role: "withdraw_disburse",
					transfer_id: tid(aggregate_id, WALLET_REDISTRIBUTE),
					action: LedgerAction::Post(LedgerTransfer {
						id: tid(aggregate_id, WALLET_REDISTRIBUTE),
						debit: LedgerAccountKey::WithdrawalClearing,
						credit: LedgerAccountKey::CryptoWallet(network),
						amount: net.base_units(),
						code: TransferCode::Withdraw,
						reference,
					}),
				},
			];
			if !fee.is_zero() {
				ops.push(PlannedOp {
					role: "withdraw_fee",
					transfer_id: tid(aggregate_id, FEE_REDISTRIBUTE),
					action: LedgerAction::Post(LedgerTransfer {
						id: tid(aggregate_id, FEE_REDISTRIBUTE),
						debit: LedgerAccountKey::WithdrawalClearing,
						credit: LedgerAccountKey::FeeRevenue,
						amount: fee.base_units(),
						code: TransferCode::WithdrawFee,
						reference,
					}),
				});
			}
			ops
		}
		WithdrawalEvent::Failed { user, amount, .. } => vec![void_clearing(aggregate_id, user, amount, reference, CLEARING_VOID_FAIL)],
		WithdrawalEvent::Cancelled { user, amount, .. } => vec![void_clearing(aggregate_id, user, amount, reference, CLEARING_VOID_CANCEL)],
	})
}

/// Void the clearing reservation (full refund) — shared by fail and cancel, which only
/// differ by the completion's deterministic id salt (a withdrawal reaches at most one).
fn void_clearing(aggregate_id: Uuid, user: UserId, amount: Usdt, reference: u128, salt: &[u8]) -> PlannedOp {
	PlannedOp {
		role: "withdraw_void",
		transfer_id: tid(aggregate_id, salt),
		action: LedgerAction::Complete(PendingCompletion {
			id: tid(aggregate_id, salt),
			pending_id: tid(aggregate_id, CLEARING_RESERVE),
			kind: CompletionKind::Void,
			debit: LedgerAccountKey::UserClaim(user),
			credit: LedgerAccountKey::WithdrawalClearing,
			amount: amount.base_units(),
			code: TransferCode::Withdraw,
			reference,
		}),
	}
}

/// A deterministic TigerBeetle transfer id for one leg/phase of a withdrawal, derived
/// from the (stable) withdrawal id + a per-leg salt, so a retried delivery recomputes
/// the same id and a completion can recompute its reservation's `pending_id`.
fn tid(aggregate_id: Uuid, salt: &[u8]) -> u128 {
	Uuid::new_v5(&aggregate_id, salt).as_u128()
}

/// Whether a withdrawal's clearing reservation actually applied to the ledger: its
/// deterministic transfer id (`tid(aggregate, CLEARING_RESERVE)`) was recorded in
/// `saga_steps` when the Requested leg posted. Strict `seq` order (single worker)
/// guarantees the Requested row was fully processed — applied or parked — before its
/// Dispatched row is reached, so a missing step means the reserve parked, not that it
/// is still pending.
async fn reserve_applied(pool: &PgPool, aggregate_id: Uuid) -> Result<bool, sqlx::Error> {
	let reserve_tid = tid(aggregate_id, CLEARING_RESERVE);
	sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM saga_steps WHERE tb_transfer_id = $1)")
		.bind(&reserve_tid.to_be_bytes()[..])
		.fetch_one(pool)
		.await
}

/// The withdrawal row's current state — the broadcast Read-First's second guard (beside
/// [`reserve_applied`]): only a `processing` withdrawal may broadcast.
async fn withdrawal_state(pool: &PgPool, aggregate_id: Uuid) -> Result<Option<String>, sqlx::Error> {
	sqlx::query_scalar("SELECT state FROM withdrawals WHERE id = $1").bind(aggregate_id).fetch_optional(pool).await
}

async fn record_saga_step(pool: &PgPool, event_id: Uuid, leg: i32, role: &str, transfer_id: u128) -> Result<(), sqlx::Error> {
	sqlx::query("INSERT INTO saga_steps (event_id, leg, role, tb_transfer_id) VALUES ($1, $2, $3, $4) ON CONFLICT (event_id, leg) DO NOTHING")
		.bind(event_id)
		.bind(leg)
		.bind(role)
		.bind(&transfer_id.to_be_bytes()[..])
		.execute(pool)
		.await?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use domain::{
		balance::ServiceId,
		money::{Nav, Shares, Usdt},
	};

	use super::*;

	// Guards the redemption settle leg order documented on the aggregate and PATTERNS:
	// burn-first (post the pending burn), payout-second. A `payout-first` regression
	// would let a raced over-redeem pay cash before a valid burn — a double-spend path.
	#[test]
	fn settled_redemption_burns_before_it_pays() {
		let aggregate_id = Uuid::new_v4();
		let user = UserId::new();
		let service = ServiceId::parse("trading").unwrap();
		let event = RedemptionEvent::Settled {
			redemption_id: domain::redemptions::RedemptionId::new(),
			user,
			service,
			units: Shares::parse_decimal("100").unwrap(),
			nav: Nav::parse_decimal("1.5").unwrap(),
			cash: Usdt::parse_decimal("150").unwrap(),
		};

		let ops = plan_redemption(event, aggregate_id, aggregate_id.as_u128());

		assert_eq!(ops.len(), 2);
		assert_eq!(ops[0].role, "redeem_burn");
		assert!(matches!(ops[0].action, LedgerAction::Complete(PendingCompletion { kind: CompletionKind::Post, .. })));
		assert_eq!(ops[1].role, "redeem_payout");
		assert!(matches!(ops[1].action, LedgerAction::Post(_)));
	}
}
