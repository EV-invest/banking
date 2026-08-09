//! Per-rail custody drift watch — does the ledger still describe the chain?
//!
//! [`reconciliation`](super::reconciliation) asserts the **global** `sum(custody) == sum(claims)`,
//! which is an identity between two TigerBeetle accounts: it holds whether or not the chain
//! agrees with either of them. Comparing a rail's `wallet:<net>` against the wallets it claims
//! to describe was left out as "the treasury's job, surfaced separately" — this is that surface.
//!
//! The gap it closes is real and was reached in production: USDT sent straight to a treasury
//! hot wallet moves money the ledger never records, and every existing check stays green,
//! because the invariant relates TB to TB. The rail's own screen showed the two numbers side by
//! side and nothing compared them.
//!
//! **Alert-only, never a write.** The house rule for reconciliation, and doubly right here: a
//! chain read can be stale, throttled or reorged, and a job that "corrected" the ledger from one
//! would post money on the strength of a flaky node. The credit path is the deposit watcher,
//! which is idempotent per transaction; this only says the two sides disagree.
//!
//! **The comparison.** `wallet:<net>` counts every USDT we control on the rail, wherever it sits
//! — the sweep moving funds from a deposit address to the treasury is invisible to it. So the
//! chain side must be the same union:
//!
//! ```text
//! expected = wallet:<net>                                    (TigerBeetle)
//! actual   = treasury balance + Σ derived deposit addresses   (chain)
//! ```
//!
//! `actual > expected` is unrecorded money arriving — the operator top-up above, or a user
//! sending to the wrong address. `actual < expected` is the serious direction: the ledger
//! believes in funds the chain cannot show, so it is reported at a louder level.
//!
//! **Races are not alerts.** Both sides are read at slightly different instants, so a transfer
//! landing between them shows up as drift that is gone a moment later. A divergence is only
//! reported once it has survived two consecutive scans with the same sign, which costs an
//! interval of latency on a real problem and removes the entire class of false alarm.

use std::{
	collections::HashMap,
	sync::{Arc, Mutex},
	time::Duration,
};

use domain::{
	balance::LedgerAccountKey,
	money::{Network, Usdt},
};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::ports::{Custody, Ledger};

/// Ignore differences at or below this — one cent in canonical 18-dp base units. Nothing in
/// the ledger math rounds, so a real drift is never this small; this only absorbs a rail whose
/// on-chain precision is coarser than canonical.
const DUST_BASE_UNITS: u128 = 10_000_000_000_000_000;

/// Hourly. The scan costs one chain read per derived deposit address per rail, against the
/// same rate-limited public endpoints the watchers share, so it is deliberately far slower
/// than [`reconciliation`](super::reconciliation)'s minute — the drift it looks for is an
/// operator action or a bug, neither of which needs sub-hour detection.
const SCAN_INTERVAL: Duration = Duration::from_secs(3600);

/// What the previous scan saw for a rail, so a divergence must persist to be reported.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Drift {
	/// Chain and ledger agreed (within dust).
	None,
	/// Chain holds more than the ledger records — unrecorded arrivals.
	Surplus,
	/// The ledger records more than the chain holds — the alarming direction.
	Shortfall,
}

pub struct TreasuryDrift {
	ledger: Arc<dyn Ledger>,
	custody: Arc<dyn Custody>,
	/// Last scan's verdict per rail — a divergence is reported only when repeated.
	previous: Mutex<HashMap<Network, Drift>>,
}

impl TreasuryDrift {
	pub fn new(ledger: Arc<dyn Ledger>, custody: Arc<dyn Custody>) -> Self {
		Self {
			ledger,
			custody,
			previous: Mutex::new(HashMap::new()),
		}
	}

	pub async fn run(self, shutdown: CancellationToken) {
		info!("treasury drift watch: comparing per-rail ledger custody against the chain every {SCAN_INTERVAL:?}");
		loop {
			// The first tick fires immediately: a process that just started is exactly when an
			// operator is most likely to be waiting to see whether a top-up registered.
			self.scan_once().await;
			tokio::select! {
				() = shutdown.cancelled() => {
					info!("treasury drift watch: shutdown requested — stopping");
					return;
				}
				() = tokio::time::sleep(SCAN_INTERVAL) => {}
			}
		}
	}

	async fn scan_once(&self) {
		for network in Network::ALL {
			// A rail with no chain view (unwired, or the stub) has nothing to compare against;
			// it is not a drift and must not be reported as one.
			let (Ok(Some(treasury)), Ok(Some(deposits))) = (self.custody.treasury_liquidity(network).await, self.custody.deposit_address_liquidity(network).await) else {
				continue;
			};
			let Some(actual) = treasury.checked_add(deposits) else {
				warn!(%network, "treasury drift watch: on-chain total overflowed — skipping this rail");
				continue;
			};
			let expected = match self.ledger.balance(&LedgerAccountKey::CryptoWallet(network)).await {
				Ok(balance) => Usdt::from_base_units(balance.posted),
				Err(err) => {
					// TigerBeetle being unreachable is its own alert elsewhere; here it just
					// means this rail cannot be judged this cycle.
					warn!(%network, "treasury drift watch: ledger balance unavailable: {err}");
					continue;
				}
			};
			self.report(network, expected, actual);
		}
	}

	/// Classify a rail and alert only when the same divergence survived the previous scan.
	fn report(&self, network: Network, expected: Usdt, actual: Usdt) {
		let drift = classify(expected, actual);
		let repeated = {
			let mut previous = self.previous.lock().expect("treasury drift state mutex poisoned");
			previous.insert(network, drift) == Some(drift)
		};
		if !repeated {
			return;
		}
		let (expected, actual) = (expected.to_decimal_string(), actual.to_decimal_string());
		match drift {
			Drift::None => {}
			// Money we hold but never booked. Recoverable and safe-side (more assets than
			// liabilities), but it is capital nobody can spend: the withdrawal dispatch gate
			// mins the ledger against the chain, so an unrecorded rail stays unspendable.
			Drift::Surplus => error!(
				%network,
				%expected,
				%actual,
				"treasury drift: the chain holds MORE USDT than the ledger records — an arrival was never credited; record it against its on-chain reference"
			),
			// The ledger promises funds the chain cannot show. Either money left without a
			// ledger fact, or a credit was posted for a transfer that never landed.
			Drift::Shortfall => error!(
				%network,
				%expected,
				%actual,
				"treasury drift: the ledger records MORE USDT than the chain holds — claims on this rail are not fully backed on-chain"
			),
		}
	}
}

/// Compare the two sides with a dust tolerance, in whichever direction they differ.
fn classify(expected: Usdt, actual: Usdt) -> Drift {
	let (expected, actual) = (expected.base_units(), actual.base_units());
	if expected.abs_diff(actual) <= DUST_BASE_UNITS {
		Drift::None
	} else if actual > expected {
		Drift::Surplus
	} else {
		Drift::Shortfall
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn usdt(whole: u128) -> Usdt {
		Usdt::from_base_units(whole * 1_000_000_000_000_000_000)
	}

	#[test]
	fn an_unrecorded_arrival_reads_as_a_surplus() {
		// The production case: 1 USDT sent to the treasury, ledger untouched.
		assert!(matches!(classify(usdt(0), usdt(1)), Drift::Surplus));
		assert!(matches!(classify(usdt(1), usdt(2)), Drift::Surplus));
	}

	#[test]
	fn unbacked_claims_read_as_a_shortfall() {
		assert!(matches!(classify(usdt(5), usdt(4)), Drift::Shortfall));
	}

	#[test]
	fn agreement_within_dust_is_not_a_drift() {
		assert!(matches!(classify(usdt(3), usdt(3)), Drift::None));
		// A cent either way is tolerated; anything coarser is not.
		let cent = Usdt::from_base_units(DUST_BASE_UNITS);
		assert!(matches!(classify(usdt(3), usdt(3).checked_add(cent).unwrap()), Drift::None));
		let over = Usdt::from_base_units(DUST_BASE_UNITS + 1);
		assert!(matches!(classify(usdt(3), usdt(3).checked_add(over).unwrap()), Drift::Surplus));
	}
}
