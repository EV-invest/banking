//! Outbox relay — the saga dispatcher.
//!
//! A single worker drains undispatched [`outbox`](super::outbox) rows in strict
//! `seq` order and applies each to TigerBeetle (and, for withdrawals, the custody
//! service) **after** the control-plane commit (Write-Last). Delivery is
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
//! rejection is **parked** (advanced past, with a loud log + `last_error`) so one bad
//! event can't wedge the whole queue — reconciliation and the `last_error` column
//! surface it for intervention.

use std::{sync::Arc, time::Duration};

use domain::{
	allocations::{AllocationEvent, AllocationKind},
	balance::{LedgerAccountKey, LedgerEvent, TransferCode},
	money::Usdt,
	users::UserId,
	withdrawals::WithdrawalEvent,
};
use sqlx::PgPool;
use tokio::sync::Notify;
use tracing::{error, warn};
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

/// Bound on consecutive `RetryBounded` attempts before a never-resolving retryable (a
/// completion whose pending was itself parked, so it can never be found) is parked
/// rather than wedging the single-worker queue forever.
const MAX_RETRYABLE_ATTEMPTS: i32 = 25;

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

	/// Run forever: drain the backlog, then wait for a nudge or a poll-fallback.
	pub async fn run(self) {
		loop {
			let backoff = self.drain().await;
			let wait = if backoff { Duration::from_secs(2) } else { Duration::from_millis(500) };
			tokio::select! {
				() = self.notify.notified() => {},
				() = tokio::time::sleep(wait) => {},
			}
		}
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
						let _ = outbox::mark_dispatched(&self.pool, row.seq).await;
						advanced = true;
					}
					Outcome::Park(reason) => {
						error!(seq = row.seq, event_id = %row.event_id, "relay: parking event (needs intervention): {reason}");
						let _ = outbox::record_failure(&self.pool, row.seq, &reason).await;
						let _ = outbox::mark_dispatched(&self.pool, row.seq).await;
						advanced = true;
					}
					Outcome::Retry(reason) => {
						warn!(seq = row.seq, "relay: transient failure (unbounded), retrying from here: {reason}");
						let _ = outbox::record_failure(&self.pool, row.seq, &reason).await;
						return true;
					}
					Outcome::RetryBounded(reason) => {
						let _ = outbox::record_failure(&self.pool, row.seq, &reason).await;
						// A retryable that can never resolve (a completion whose pending was
						// itself parked, so it is never found) must not wedge the single-worker
						// queue forever — park it after a bound so later events keep flowing.
						if row.attempts + 1 >= MAX_RETRYABLE_ATTEMPTS {
							error!(seq = row.seq, event_id = %row.event_id, attempts = row.attempts + 1, "relay: parking retryable after bound (wedge guard): {reason}");
							let _ = outbox::mark_dispatched(&self.pool, row.seq).await;
							advanced = true;
						} else {
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

	async fn dispatch(&self, row: &OutboxRow) -> Outcome {
		let ops = match plan(row) {
			Ok(ops) => ops,
			Err(reason) => return Outcome::Park(format!("unplannable event: {reason}")),
		};
		// A withdrawal settle disburses the net out of a rail's custody (`Cr wallet:<net>`,
		// the rail-liquidity backstop). The relay is single-worker and sequential, so a
		// Read-First check here guarantees that op cannot fail with InsufficientFunds *after*
		// the clearing pending has already been posted — which would debit the user with the
		// gross stranded in clearing and no compensating path. A short rail parks the settle
		// atomically (nothing applied); a top-up + reconciliation recovers it.
		for op in &ops {
			if op.role == "withdraw_disburse" {
				if let LedgerAction::Post(transfer) = &op.action {
					match self.ledger.balance(&transfer.credit).await {
						Ok(balance) if balance.posted < transfer.amount => {
							return Outcome::Park(format!("rail {} liquidity insufficient at settle", transfer.credit.logical_key()));
						}
						Ok(_) => {}
						Err(LedgerError::Unavailable(err)) => return Outcome::Retry(err),
						Err(LedgerError::Retryable(err)) => return Outcome::RetryBounded(err),
						Err(err) => return Outcome::Park(format!("settle rail check: {err}")),
					}
				}
			}
		}
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
					if !matches!(op.action, LedgerAction::Broadcast(_)) {
						let _ = record_saga_step(&self.pool, row.event_id, leg as i32, op.role, op.transfer_id).await;
					}
				}
				Err(LedgerError::Unavailable(err)) => return Outcome::Retry(err),
				Err(LedgerError::Retryable(err)) => return Outcome::RetryBounded(err),
				Err(LedgerError::InsufficientFunds) => return Outcome::Park("insufficient funds".into()),
				Err(LedgerError::Conflict(err)) => return Outcome::Park(format!("ledger conflict: {err}")),
			}
		}
		Outcome::Done
	}
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
	/// Terminal for this event — advance past it with a loud log (a discrepancy
	/// reconciliation will catch; auto-compensation is a follow-up).
	Park(String),
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
		"allocations" => {
			let event: AllocationEvent = serde_json::from_str(&row.payload).map_err(|e| e.to_string())?;
			Ok(vec![plan_allocation(event, event_tid, pending_transfer_id(row.aggregate_id), reference)])
		}
		"withdrawals" => {
			let event: WithdrawalEvent = serde_json::from_str(&row.payload).map_err(|e| e.to_string())?;
			Ok(plan_withdrawal(event, row.aggregate_id, reference))
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
				amount,
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
				amount,
				code: TransferCode::SeedCapital,
				reference,
			}),
		},
	}
}

fn plan_allocation(event: AllocationEvent, event_tid: u128, pending_tid: u128, reference: u128) -> PlannedOp {
	match event {
		AllocationEvent::Opened { amount, kind, .. } => match kind {
			AllocationKind::UserStake { user, service } => PlannedOp {
				role: "allocate",
				transfer_id: event_tid,
				action: LedgerAction::Post(LedgerTransfer {
					id: event_tid,
					debit: LedgerAccountKey::UserClaim(user),
					credit: LedgerAccountKey::ServiceClaim(service),
					amount,
					code: TransferCode::UserAllocate,
					reference,
				}),
			},
			AllocationKind::ServiceReservation { service } => PlannedOp {
				role: "reserve",
				transfer_id: pending_tid,
				action: LedgerAction::Reserve(LedgerTransfer {
					id: pending_tid,
					debit: LedgerAccountKey::Fund,
					credit: LedgerAccountKey::ServiceClaim(service),
					amount,
					code: TransferCode::ServiceReserve,
					reference,
				}),
			},
			AllocationKind::ServiceHolding { service } => PlannedOp {
				role: "transfer",
				transfer_id: event_tid,
				action: LedgerAction::Post(LedgerTransfer {
					id: event_tid,
					debit: LedgerAccountKey::Fund,
					credit: LedgerAccountKey::ServiceClaim(service),
					amount,
					code: TransferCode::ServiceTransfer,
					reference,
				}),
			},
		},
		AllocationEvent::Revoked { amount, user, service, .. } => PlannedOp {
			role: "revoke",
			transfer_id: event_tid,
			action: LedgerAction::Post(LedgerTransfer {
				id: event_tid,
				debit: LedgerAccountKey::ServiceClaim(service),
				credit: LedgerAccountKey::UserClaim(user),
				amount,
				code: TransferCode::UserRevoke,
				reference,
			}),
		},
		AllocationEvent::Settled { amount, service, .. } => PlannedOp {
			role: "settle",
			transfer_id: event_tid,
			action: LedgerAction::Complete(PendingCompletion {
				id: event_tid,
				pending_id: pending_tid,
				kind: CompletionKind::Post,
				debit: LedgerAccountKey::Fund,
				credit: LedgerAccountKey::ServiceClaim(service),
				amount,
				code: TransferCode::ServiceSettle,
				reference,
			}),
		},
		AllocationEvent::Cancelled { amount, service, .. } => PlannedOp {
			role: "cancel",
			transfer_id: event_tid,
			action: LedgerAction::Complete(PendingCompletion {
				id: event_tid,
				pending_id: pending_tid,
				kind: CompletionKind::Void,
				debit: LedgerAccountKey::Fund,
				credit: LedgerAccountKey::ServiceClaim(service),
				amount,
				code: TransferCode::ServiceCancel,
				reference,
			}),
		},
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
fn plan_withdrawal(event: WithdrawalEvent, aggregate_id: Uuid, reference: u128) -> Vec<PlannedOp> {
	match event {
		WithdrawalEvent::Requested { user, amount, .. } => vec![PlannedOp {
			role: "withdraw_reserve",
			transfer_id: tid(aggregate_id, CLEARING_RESERVE),
			action: LedgerAction::Reserve(LedgerTransfer {
				id: tid(aggregate_id, CLEARING_RESERVE),
				debit: LedgerAccountKey::UserClaim(user),
				credit: LedgerAccountKey::WithdrawalClearing,
				amount,
				code: TransferCode::Withdraw,
				reference,
			}),
		}],
		WithdrawalEvent::Dispatched { network, address, amount, fee, .. } => {
			let net = amount.checked_sub(fee).unwrap_or(Usdt::ZERO);
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
			let net = amount.checked_sub(fee).unwrap_or(Usdt::ZERO);
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
						amount,
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
						amount: net,
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
						amount: fee,
						code: TransferCode::WithdrawFee,
						reference,
					}),
				});
			}
			ops
		}
		WithdrawalEvent::Failed { user, amount, .. } => vec![void_clearing(aggregate_id, user, amount, reference, CLEARING_VOID_FAIL)],
		WithdrawalEvent::Cancelled { user, amount, .. } => vec![void_clearing(aggregate_id, user, amount, reference, CLEARING_VOID_CANCEL)],
	}
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
			amount,
			code: TransferCode::Withdraw,
			reference,
		}),
	}
}

/// The stable id of a reservation's pending transfer — derived from the allocation
/// id (not the event id) so the later settle/cancel can recompute it as `pending_id`.
fn pending_transfer_id(aggregate_id: Uuid) -> u128 {
	Uuid::new_v5(&aggregate_id, b"reserve").as_u128()
}

/// A deterministic TigerBeetle transfer id for one leg/phase of a withdrawal, derived
/// from the (stable) withdrawal id + a per-leg salt, so a retried delivery recomputes
/// the same id and a completion can recompute its reservation's `pending_id`.
fn tid(aggregate_id: Uuid, salt: &[u8]) -> u128 {
	Uuid::new_v5(&aggregate_id, salt).as_u128()
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
