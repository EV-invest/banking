//! Outbox relay — the saga dispatcher.
//!
//! A single worker drains undispatched [`outbox`](super::outbox) rows in strict
//! `seq` order and applies each to TigerBeetle **after** the control-plane commit
//! (Write-Last). Delivery is at-least-once, so every op is idempotent: transfer ids
//! are deterministic (the event id; a reservation's pending uses an
//! aggregate-derived id so its completion can reference it), and the gateway treats
//! `Exists`/`AlreadyPosted`/`AlreadyVoided` as success.
//!
//! Strict order means a reservation's pending is always applied before its
//! settle/cancel, so a two-phase completion never races its own pending. Transient
//! failures (`Unavailable`/`Retryable`) stop the cycle and retry from the same `seq`;
//! a genuine inconsistency or `InsufficientFunds` is **parked** (advanced past, with
//! a loud log + `last_error`) so one bad event can't wedge the whole queue —
//! reconciliation and the `last_error` column surface it for intervention.

use std::{sync::Arc, time::Duration};

use domain::{
	allocations::{AllocationEvent, AllocationKind},
	balance::{LedgerAccountKey, LedgerEvent, TransferCode},
};
use sqlx::PgPool;
use tokio::sync::Notify;
use tracing::{error, warn};
use uuid::Uuid;

use crate::{
	infrastructure::outbox::{self, OutboxRow},
	ports::ledger::{CompletionKind, Ledger, LedgerError, LedgerTransfer, PendingCompletion},
};

/// The relay task: drains the outbox to the ledger. Cloneable handles (`pool`,
/// `ledger`, `notify`) so command handlers can `notify` it to dispatch promptly.
pub struct Relay {
	pool: PgPool,
	ledger: Arc<dyn Ledger>,
	notify: Arc<Notify>,
}

impl Relay {
	pub fn new(pool: PgPool, ledger: Arc<dyn Ledger>, notify: Arc<Notify>) -> Self {
		Self { pool, ledger, notify }
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
						warn!(seq = row.seq, "relay: transient failure, retrying from here: {reason}");
						let _ = outbox::record_failure(&self.pool, row.seq, &reason).await;
						return true;
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
		for (leg, op) in ops.iter().enumerate() {
			let result = match &op.action {
				LedgerAction::Post(transfer) => self.ledger.post(transfer).await,
				LedgerAction::Reserve(transfer) => self.ledger.reserve(transfer).await,
				LedgerAction::Complete(completion) => self.ledger.complete(completion).await,
			};
			match result {
				Ok(()) => {
					let _ = record_saga_step(&self.pool, row.event_id, leg as i32, op.role, op.transfer_id).await;
				}
				Err(LedgerError::Unavailable(err)) | Err(LedgerError::Retryable(err)) => return Outcome::Retry(err),
				Err(LedgerError::InsufficientFunds) => return Outcome::Park("insufficient funds".into()),
				Err(LedgerError::Conflict(err)) => return Outcome::Park(format!("ledger conflict: {err}")),
			}
		}
		Outcome::Done
	}
}

enum Outcome {
	Done,
	/// Transient — stop and retry from this `seq` (TB unreachable / pending not yet visible).
	Retry(String),
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
}

/// Map an outbox event to the ledger op(s) it performs. `reference` (stamped into
/// `user_data_128`) is always the aggregate id, tying every TB transfer back to its
/// allocation/deposit for reconciliation.
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
				credit: party.claim_key(network),
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
				credit: LedgerAccountKey::Fund(network),
				amount,
				code: TransferCode::SeedCapital,
				reference,
			}),
		},
	}
}

fn plan_allocation(event: AllocationEvent, event_tid: u128, pending_tid: u128, reference: u128) -> PlannedOp {
	match event {
		AllocationEvent::Opened { amount, network, kind, .. } => match kind {
			AllocationKind::UserStake { user, service } => PlannedOp {
				role: "allocate",
				transfer_id: event_tid,
				action: LedgerAction::Post(LedgerTransfer {
					id: event_tid,
					debit: LedgerAccountKey::UserClaim(user, network),
					credit: LedgerAccountKey::ServiceClaim(service, network),
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
					debit: LedgerAccountKey::Fund(network),
					credit: LedgerAccountKey::ServiceClaim(service, network),
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
					debit: LedgerAccountKey::Fund(network),
					credit: LedgerAccountKey::ServiceClaim(service, network),
					amount,
					code: TransferCode::ServiceTransfer,
					reference,
				}),
			},
		},
		AllocationEvent::Revoked { amount, network, user, service, .. } => PlannedOp {
			role: "revoke",
			transfer_id: event_tid,
			action: LedgerAction::Post(LedgerTransfer {
				id: event_tid,
				debit: LedgerAccountKey::ServiceClaim(service, network),
				credit: LedgerAccountKey::UserClaim(user, network),
				amount,
				code: TransferCode::UserRevoke,
				reference,
			}),
		},
		AllocationEvent::Settled { amount, network, service, .. } => PlannedOp {
			role: "settle",
			transfer_id: event_tid,
			action: LedgerAction::Complete(PendingCompletion {
				id: event_tid,
				pending_id: pending_tid,
				kind: CompletionKind::Post,
				debit: LedgerAccountKey::Fund(network),
				credit: LedgerAccountKey::ServiceClaim(service, network),
				amount,
				code: TransferCode::ServiceSettle,
				reference,
			}),
		},
		AllocationEvent::Cancelled { amount, network, service, .. } => PlannedOp {
			role: "cancel",
			transfer_id: event_tid,
			action: LedgerAction::Complete(PendingCompletion {
				id: event_tid,
				pending_id: pending_tid,
				kind: CompletionKind::Void,
				debit: LedgerAccountKey::Fund(network),
				credit: LedgerAccountKey::ServiceClaim(service, network),
				amount,
				code: TransferCode::ServiceCancel,
				reference,
			}),
		},
	}
}

/// The stable id of a reservation's pending transfer — derived from the allocation
/// id (not the event id) so the later settle/cancel can recompute it as `pending_id`.
fn pending_transfer_id(aggregate_id: Uuid) -> u128 {
	Uuid::new_v5(&aggregate_id, b"reserve").as_u128()
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
