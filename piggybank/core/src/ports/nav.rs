//! The fund-valuation (NAV) port — the operator-posted AUM marks a fund's NAV is
//! derived from.
//!
//! NAV per share = `AUM / units_outstanding`, derived at post time and frozen until the
//! next mark. This port stores the append-only marks and returns the latest (the
//! "current" price subscribe/redeem deal on). Plain control-plane reads/writes — not an
//! event-sourced aggregate — so a simple trait like
//! [`DepositAddresses`](super::DepositAddresses), not a `Repository`.

use async_trait::async_trait;
use domain::{
	balance::ServiceId,
	error::DomainError,
	money::{Nav, Shares, Usdt},
};
use uuid::Uuid;

#[async_trait]
pub trait NavRepository: Send + Sync {
	/// The latest mark for `service`, or `None` if the fund has never been valued.
	async fn current(&self, service: &ServiceId) -> Result<Option<Valuation>, DomainError>;

	/// Append a new mark — `id` is caller-minted, `posted_at` is DB-stamped. Returns the
	/// stamped `posted_at` (unix seconds) so the caller can report the recorded mark.
	async fn record(&self, id: Uuid, service: &ServiceId, aum: Usdt, units_outstanding: Shares, nav: Nav, posted_by: &str) -> Result<i64, DomainError>;

	/// All marks for `service`, newest first (admin/UI history).
	async fn history(&self, service: &ServiceId) -> Result<Vec<Valuation>, DomainError>;
}
/// One operator valuation mark for a fund. NAV is derived (`aum / units_outstanding`)
/// and frozen until the next mark; `posted_at_unix` is the age seam for the staleness
/// guard, `posted_by` the operator subject (the trust seam).
#[derive(Debug, Clone)]
pub struct Valuation {
	pub service: ServiceId,
	pub aum: Usdt,
	pub units_outstanding: Shares,
	pub nav: Nav,
	pub posted_by: String,
	pub posted_at_unix: i64,
}
