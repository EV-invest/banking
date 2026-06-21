//! Read port for the per-investor fund-position projection.
//!
//! Units are authoritative in TigerBeetle; this projection carries the control-plane
//! extras a unit balance can't — the average-cost basis (for P&L) and the per-investor
//! NAV high-water mark (reserved for a future performance fee). Read-only here:
//! subscribe/redeem maintain it. The application joins it with the live unit balance and
//! current NAV to build a position view.

use async_trait::async_trait;
use domain::{
	balance::ServiceId,
	error::DomainError,
	money::{Nav, Usdt},
	users::UserId,
};

#[async_trait]
pub trait FundPositionReader: Send + Sync {
	/// The caller's position projection for one fund, or `None` if they have none.
	async fn find(&self, user: UserId, service: &ServiceId) -> Result<Option<FundPosition>, DomainError>;

	/// All of the caller's position projections.
	async fn list(&self, user: UserId) -> Result<Vec<FundPosition>, DomainError>;
}
/// A per-(user, service) position projection.
#[derive(Debug, Clone)]
pub struct FundPosition {
	pub service: ServiceId,
	/// Net cash invested (average cost), for P&L against the current value.
	pub cost_basis: Usdt,
	/// The highest NAV the investor has subscribed at — reserved for a performance fee.
	pub high_water_mark: Nav,
}
