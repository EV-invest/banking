//! Stub custody adapter — a stand-in for the real signing/custody service.
//!
//! It performs NO real signing or broadcast: it logs the request and returns success
//! (a no-op is trivially idempotent by `withdrawal_id`). The synthetic on-chain
//! reference an operator later settles with is theirs to supply via
//! `BalanceService.SettleWithdrawal`. Swap this for the MPC/HSM adapter when the
//! custody service lands; the [`Custody`] port, the relay, and the saga all stay put.

use async_trait::async_trait;
use tracing::info;

use crate::ports::custody::{BroadcastRequest, Custody, CustodyError};

pub struct StubCustody;

#[async_trait]
impl Custody for StubCustody {
	async fn broadcast(&self, request: &BroadcastRequest) -> Result<(), CustodyError> {
		info!(
			withdrawal_id = %request.withdrawal_id,
			network = %request.network,
			address = request.address.as_str(),
			amount = %request.amount,
			"stub custody: pretending to broadcast a withdrawal (no real chain); awaiting operator settle/fail"
		);
		Ok(())
	}
}
