//! The **Allocation** contract — the curated entry point for every conversation
//! about an investable product.
//!
//! A consumer service repo imports this one module instead of hunting through the
//! generated `banking::v1` namespace: it re-exports the allocation stubs and message
//! types, and pins the wire vocabularies the two sides must agree on ([`state`],
//! [`icon`]).
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

/// The canonical `Allocation.icon` strings.
///
/// The other half of the vocabulary contract, alongside [`state`]: the hub's
/// `domain::allocations::AllocationIcon` serializes to exactly these, and a client
/// picks its SVG by matching one of these constants rather than its own literals.
///
/// Presentation only — nothing here gates money or lifecycle, and the hub never
/// branches on it. It exists so a product's card is a decision an operator made rather
/// than a letter avatar derived from the title.
///
/// Adding one means four edits, and each has a test standing behind it: here and in
/// [`ALL`] (`domain_icons_match_the_wire_contract` compares this list with the hub's
/// `AllocationIcon::ALL` member for member, both ways), the domain enum (its `next`
/// chain makes the compiler insist), a migration widening the `allocations.icon` CHECK
/// (`every_icon_the_domain_knows_is_accepted_by_the_column` writes every variant into
/// the column), and the client that has to draw it.
///
/// Ship the client that knows a new value BEFORE the hub that can store it: a client
/// reading one it does not know falls back to [`DEFAULT`], and so does the hub reading
/// such a row back out of storage, so neither direction can be brought down by a
/// half-rolled deployment.
pub mod icon {
	/// The neutral default. A registration that names no icon lands here, and so does
	/// every row written before the field existed.
	pub const FUND: &str = "fund";
	pub const REAL_ESTATE: &str = "real_estate";
	pub const TRADING: &str = "trading";
	pub const YIELD: &str = "yield";
	pub const VENTURE: &str = "venture";
	pub const TREASURY: &str = "treasury";
	pub const COMMODITY: &str = "commodity";
	pub const CREDIT: &str = "credit";
	pub const INDEX: &str = "index";
	pub const ARBITRAGE: &str = "arbitrage";

	/// Every icon. The order is the one an operator's picker shows.
	pub const ALL: [&str; 10] = [FUND, REAL_ESTATE, TRADING, YIELD, VENTURE, TREASURY, COMMODITY, CREDIT, INDEX, ARBITRAGE];

	/// The icon a product carries until an operator picks one.
	pub const DEFAULT: &str = FUND;

	/// Whether `icon` is one this contract defines. A client rendering an icon it does
	/// not know should fall back to [`DEFAULT`]; a client *sending* one is refused.
	pub fn is_known(icon: &str) -> bool {
		ALL.contains(&icon)
	}
}

#[cfg(test)]
mod tests {
	use super::{icon, state};

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

	#[test]
	fn the_icon_vocabulary_is_closed_and_canonical() {
		// Byte-identical with `domain::allocations::AllocationIcon::as_str`
		// (`domain_icons_match_the_wire_contract` guards the other side), and the exact
		// set the client ships artwork for.
		assert_eq!(
			icon::ALL,
			["fund", "real_estate", "trading", "yield", "venture", "treasury", "commodity", "credit", "index", "arbitrage"]
		);
		assert!(icon::ALL.iter().all(|i| icon::is_known(i)));
		// A client that can send a value nothing draws has no closed vocabulary at all.
		assert!(!icon::is_known("rocket"));
		assert!(!icon::is_known(""));
		assert!(!icon::is_known("realEstate"), "the wire form is lowercase snake_case");
	}

	#[test]
	fn the_default_icon_is_a_member_of_the_set() {
		// The hub lands an un-iconed registration here, so a client that cannot draw it
		// would leave every fresh product blank.
		assert_eq!(icon::DEFAULT, icon::FUND);
		assert!(icon::is_known(icon::DEFAULT));
	}
}
