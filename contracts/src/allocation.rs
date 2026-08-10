//! The **Allocation** contract — the curated entry point for every conversation
//! about an investable product.
//!
//! A consumer service repo imports this one module instead of hunting through the
//! generated `banking::v1` namespace: it re-exports the allocation stubs and message
//! types, and pins the wire vocabulary the two sides must agree on ([`state`]).
//!
//! # The conversation
//!
//! ```text
//! operator ─ RegisterAllocation ─▶ draft    (registered, accepts no money)
//! operator ─ SetAllocationState ─▶ open     (subscribe + redeem)
//! investor ─ ListAllocations ────▶ the catalog (open only, unless AllocationManage)
//! investor ─ FundsService.Subscribe ────▶ refused unless the allocation is open
//! investor ─ FundsService.Redeem ───────▶ allowed while open OR closed
//! operator ─ SetAllocationUnitCap ▶ supply  (how many units may ever be issued)
//! operator ─ SetAllocationState ─▶ closed   (redeem only — never traps an investor)
//! ```
//!
//! The registry is the **gate**, twice over. `Subscribe` resolves its `service` against
//! an allocation and refuses an unregistered or non-`open` one, so an investable product
//! exists only because an `AllocationManage` holder said so — and then refuses a mint
//! that would carry the issued supply past [`Allocation::unit_cap`], so a product is
//! also only ever as large as an operator sized it.
//!
//! Money is deliberately absent here. Units, NAV, positions and cash all belong to
//! [`FundsService`](crate::banking::v1::funds_service_client::FundsServiceClient) and
//! `BalanceService`, keyed by the same `service` slug this registry owns.

pub use crate::banking::v1::{
	Allocation, AllocationList, GetAllocationRequest, ListAllocationsRequest, RegisterAllocationRequest, SetAllocationStateRequest, SetAllocationUnitCapRequest, UpdateAllocationRequest,
	allocations_service_client::AllocationsServiceClient,
	allocations_service_server::{AllocationsService, AllocationsServiceServer},
};

/// The unit cap a `RegisterAllocation` lands on, as its wire decimal — 100,000,000
/// units. A consumer rendering "of the cap" before the operator has sized the product
/// is looking at this number; it is finite on purpose, so "unlimited" is never a state
/// the registry can be in.
pub const DEFAULT_UNIT_CAP: &str = "100000000";

/// The canonical `Allocation.state` strings.
///
/// This is the cross-repo half of the contract: the hub's `domain::allocations::
/// AllocationState` serializes to exactly these, and a consumer that matches on the
/// wire string matches on these constants rather than its own literals. Adding a state
/// means adding it here and to [`ALL`].
pub mod state {
	/// Registered, but accepting no money yet — invisible in the default catalog.
	pub const DRAFT: &str = "draft";
	/// Accepting subscriptions and redemptions.
	pub const OPEN: &str = "open";
	/// Wound down: redemptions still settle, new subscriptions are refused.
	pub const CLOSED: &str = "closed";

	/// Every state, in lifecycle order.
	pub const ALL: [&str; 3] = [DRAFT, OPEN, CLOSED];

	/// Whether `state` is one this contract defines.
	pub fn is_known(state: &str) -> bool {
		ALL.contains(&state)
	}

	/// Whether an allocation in `state` accepts new money. The authoritative check
	/// still runs hub-side on `Subscribe`; this is for clients rendering the surface.
	pub fn accepts_subscriptions(state: &str) -> bool {
		state == OPEN
	}

	/// Whether an allocation in `state` still lets an investor exit. A closed product
	/// must — locking redemptions would strand real money.
	pub fn accepts_redemptions(state: &str) -> bool {
		state == OPEN || state == CLOSED
	}
}

#[cfg(test)]
mod tests {
	use super::state;

	#[test]
	fn closed_allocations_still_let_investors_exit() {
		assert!(!state::accepts_subscriptions(state::CLOSED));
		assert!(state::accepts_redemptions(state::CLOSED));
		// A draft has never taken money, so neither direction applies.
		assert!(!state::accepts_subscriptions(state::DRAFT));
		assert!(!state::accepts_redemptions(state::DRAFT));
	}

	#[test]
	fn the_default_unit_cap_matches_the_hub() {
		// Byte-identical with `domain::allocations::DEFAULT_UNIT_CAP` rendered to the wire
		// (`the_wire_default_cap_matches_the_domain` guards the other side).
		assert_eq!(super::DEFAULT_UNIT_CAP, "100000000");
	}

	#[test]
	fn unknown_states_are_rejected() {
		assert!(state::ALL.iter().all(|s| state::is_known(s)));
		assert!(!state::is_known("delisted"));
	}
}
