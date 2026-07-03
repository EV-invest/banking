//! Withdrawal dispatcher — the treasury worker that drains the accept-and-queue
//! backlog. A withdrawal on a rail short of liquidity is accepted and left `queued`
//! (PATTERNS: never refused); until now only the admin `DispatchWithdrawal` RPC ever
//! re-dispatched one. This periodic job is the automatic path: each sweep re-checks
//! every `queued` withdrawal against **both** liquidity gates — the TigerBeetle rail
//! accounting balance AND the custody adapter's real on-chain treasury view — and
//! dispatches the ones both cover, so a rail top-up self-heals the queue within one
//! interval. The reaper's 24h auto-cancel of `queued` withdrawals remains the final
//! backstop (the de-facto rail top-up SLA).

use std::{sync::Arc, time::Duration};

use domain::{balance::LedgerAccountKey, money::Usdt, withdrawals::WithdrawalId};
use sqlx::PgPool;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
	application::withdrawals as withdrawal_app,
	ports::{Custody, WithdrawalRepository, ledger::Ledger},
};

/// How often the dispatcher re-checks the queued backlog. Well inside the reaper's
/// abandonment window, so any top-up within the SLA wins the race against auto-cancel.
const DISPATCH_INTERVAL: Duration = Duration::from_secs(30);

/// The dispatcher job: sweep on an interval until the process exits.
pub struct Dispatcher {
	pool: PgPool,
	withdrawals: Arc<dyn WithdrawalRepository>,
	ledger: Arc<dyn Ledger>,
	custody: Arc<dyn Custody>,
	notify: Arc<Notify>,
}

impl Dispatcher {
	pub fn new(pool: PgPool, withdrawals: Arc<dyn WithdrawalRepository>, ledger: Arc<dyn Ledger>, custody: Arc<dyn Custody>, notify: Arc<Notify>) -> Self {
		Self {
			pool,
			withdrawals,
			ledger,
			custody,
			notify,
		}
	}

	pub async fn run(self, shutdown: CancellationToken) {
		info!("dispatcher: sweeping queued withdrawals every {DISPATCH_INTERVAL:?}");
		loop {
			match self.sweep().await {
				Ok(dispatched) if dispatched > 0 => info!(dispatched, "dispatcher: dispatched queued withdrawals onto their topped-up rails"),
				Ok(_) => {}
				Err(err) => warn!("dispatcher: sweep failed (will retry): {err}"),
			}
			tokio::select! {
				() = shutdown.cancelled() => return,
				() = tokio::time::sleep(DISPATCH_INTERVAL) => {},
			}
		}
	}

	/// One sweep; returns how many withdrawals it dispatched. Public so an integration
	/// test can drive it deterministically. Dispatch is via the same application command
	/// the admin RPC uses (row-locked, idempotent — [`domain::withdrawals::Withdrawal::dispatch`]
	/// no-ops from `Processing`), so a raced operator dispatch is harmless. Per-withdrawal
	/// failures warn-and-continue; only the backlog read itself fails the sweep.
	pub async fn sweep(&self) -> Result<usize, sqlx::Error> {
		let queued: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM withdrawals WHERE state = 'queued'").fetch_all(&self.pool).await?;
		let mut dispatched = 0usize;
		for id in queued {
			let id = WithdrawalId::from_raw(id);
			let withdrawal = match self.withdrawals.find_by_id(id).await {
				Ok(Some(withdrawal)) => withdrawal,
				// Raced a terminal transition (cancel/reap) — nothing to dispatch.
				Ok(None) => continue,
				Err(err) => {
					warn!(withdrawal_id = %id, "dispatcher: could not load the queued withdrawal: {err}");
					continue;
				}
			};
			let net = withdrawal.net_amount();
			let network = withdrawal.network();
			// Gate 1 — the TB rail accounting balance must cover the net.
			match self.ledger.balance(&LedgerAccountKey::CryptoWallet(network)).await {
				Ok(balance) if Usdt::from_base_units(balance.posted) >= net => {}
				Ok(_) => continue,
				Err(err) => {
					warn!(withdrawal_id = %id, %network, "dispatcher: rail balance read failed — skipping this cycle: {err}");
					continue;
				}
			}
			// Gate 2 — the real on-chain treasury must cover it too (when there is a chain
			// view). Unlike the operator RPC, an `Err` here has no human judgment behind it,
			// so the automatic path stays conservative: skip and retry next interval.
			match self.custody.treasury_liquidity(network).await {
				Ok(None) => {}
				Ok(Some(onchain)) if onchain >= net => {}
				Ok(Some(_)) => continue,
				Err(err) => {
					warn!(withdrawal_id = %id, %network, "dispatcher: treasury liquidity read failed — skipping this cycle: {err}");
					continue;
				}
			}
			match withdrawal_app::dispatch_withdrawal(self.withdrawals.as_ref(), self.custody.as_ref(), &self.notify, id).await {
				Ok(_) => dispatched += 1,
				Err(err) => warn!(withdrawal_id = %id, "dispatcher: could not dispatch: {err}"),
			}
		}
		Ok(dispatched)
	}
}
