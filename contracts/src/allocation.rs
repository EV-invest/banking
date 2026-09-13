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
//! operator ─ RegisterAllocation ─▶ draft, access `view` (visible, locked)
//! operator ─ SetAllocationState ─▶ open     (subscribe + redeem)
//! operator ─ SetAllocationAccess ▶ invest   (anyone may put money in) — or
//! operator ─ GrantAllocationAccess ▶ one investor raised to `view` | `invest`
//! investor ─ ListAllocations ────▶ the catalog (open + at least `view` for the caller)
//! investor ─ FundsService.Subscribe ────▶ refused unless open AND `invest` for the caller
//! investor ─ FundsService.Redeem ───────▶ allowed while open OR closed, at ANY access
//! operator ─ SetAllocationUnitCap ▶ supply  (how many units may ever be issued)
//! operator ─ RevokeAllocationAccess ▶ the investor falls back to the default
//! operator ─ SetAllocationState ─▶ closed   (redeem only — never traps an investor)
//! ```
//!
//! The registry is the **gate**, three times over. `Subscribe` resolves its `service`
//! against an allocation and refuses an unregistered or non-`open` one, so an investable
//! product exists only because an `AllocationManage` holder said so; refuses a caller
//! whose effective [`access`] level is below `invest`, so a product deals only with the
//! investors an operator let in; and refuses a mint that would carry the issued supply
//! past [`Allocation::unit_cap`], so a product is also only ever as large as an operator
//! sized it. The access gate is on the way IN only — `Redeem` never consults it.
//!
//! Money is deliberately absent here. Units, NAV, positions and cash all belong to
//! [`FundsService`](crate::banking::v1::funds_service_client::FundsServiceClient) and
//! `BalanceService`, keyed by the same `service` slug this registry owns.

pub use crate::banking::v1::{
	Allocation, AllocationAccessGrant, AllocationAccessGrantList, AllocationList, GetAllocationRequest, GrantAllocationAccessRequest, ListAllocationAccessGrantsRequest,
	ListAllocationsRequest, RegisterAllocationRequest, RevokeAllocationAccessRequest, RevokeAllocationAccessResponse, SetAllocationAccessRequest, SetAllocationStateRequest,
	SetAllocationUnitCapRequest, UpdateAllocationRequest,
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

/// The canonical `Allocation.access` / `Allocation.caller_access` /
/// `AllocationAccessGrant.level` strings.
///
/// The second axis of the contract, orthogonal to [`state`]: the hub's
/// `domain::allocations::AllocationAccess` serializes to exactly these
/// (`allocation_access_strings_are_canonical` and
/// `domain_access_levels_match_the_wire_contract` guard the two sides), and a consumer
/// matches on these constants rather than its own literals. Adding a level means adding
/// it here and to [`ALL`], in rank order.
///
/// The levels are **ranked**: `hidden < view < invest`. An investor's effective level is
/// the higher of the product's default and their own grant, which is why a grant can
/// carry only [`GRANTABLE`] values — a grant only ever adds.
pub mod access {
	/// Not in the caller's catalog at all; `GetAllocation` answers NOT_FOUND.
	pub const HIDDEN: &str = "hidden";
	/// Listed and readable, but `Subscribe` is refused. What a registration lands on.
	pub const VIEW: &str = "view";
	/// Listed, readable and open to new money (subject to `state` and the unit cap).
	pub const INVEST: &str = "invest";

	/// Every level, lowest first.
	pub const ALL: [&str; 3] = [HIDDEN, VIEW, INVEST];

	/// The levels a per-investor grant may carry — everything above the floor.
	pub const GRANTABLE: [&str; 2] = [VIEW, INVEST];

	/// The level a product carries from registration until an operator changes it.
	/// "Closed by default": visible, so the catalog shows what is coming, but locked.
	pub const DEFAULT: &str = VIEW;

	/// Whether `level` is one this contract defines.
	pub fn is_known(level: &str) -> bool {
		ALL.contains(&level)
	}

	/// Whether `level` may be carried by a grant. `hidden` is refused: a grant that
	/// lowered an investor below the default would make "revoke" ambiguous.
	pub fn is_grantable(level: &str) -> bool {
		GRANTABLE.contains(&level)
	}

	/// Whether an allocation at `level` shows in the caller's catalog. The hub applies
	/// the same rule; this is for a client deciding whether to draw a card it cached.
	pub fn permits_viewing(level: &str) -> bool {
		level == VIEW || level == INVEST
	}

	/// Whether `level` lets the caller put money in — necessary alongside
	/// [`super::state::accepts_subscriptions`], never sufficient on its own. The
	/// authoritative check runs hub-side on `Subscribe`.
	pub fn permits_investing(level: &str) -> bool {
		level == INVEST
	}
}

/// The canonical `Allocation.icon` strings.
///
/// The third vocabulary of the contract, alongside [`state`] and [`access`]: the hub's
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
	use super::{access, icon, state};

	#[test]
	fn the_access_vocabulary_is_ranked_closed_and_canonical() {
		// Byte-identical with `domain::allocations::AllocationAccess::as_str`, and in the
		// rank order the hub compares by (`domain_access_levels_match_the_wire_contract`
		// guards the other side).
		assert_eq!(access::ALL, ["hidden", "view", "invest"]);
		assert!(access::ALL.iter().all(|l| access::is_known(l)));
		assert!(!access::is_known("public"));
		assert!(!access::is_known(""));
		assert!(!access::is_known("Invest"), "the wire form is lowercase");
	}

	#[test]
	fn a_registration_lands_visible_but_locked() {
		// "Closed by default" is the whole feature: the default must let the catalog show
		// the product and must NOT let money in.
		assert_eq!(access::DEFAULT, access::VIEW);
		assert!(access::permits_viewing(access::DEFAULT));
		assert!(!access::permits_investing(access::DEFAULT));
	}

	#[test]
	fn a_grant_can_only_ever_add() {
		// Every grantable level is a real level, and `hidden` is not among them.
		assert!(access::GRANTABLE.iter().all(|l| access::is_known(l)));
		assert!(access::is_grantable(access::VIEW));
		assert!(access::is_grantable(access::INVEST));
		assert!(!access::is_grantable(access::HIDDEN));
	}

	#[test]
	fn hidden_permits_nothing_and_invest_permits_everything() {
		assert!(!access::permits_viewing(access::HIDDEN));
		assert!(!access::permits_investing(access::HIDDEN));
		assert!(access::permits_viewing(access::INVEST));
		assert!(access::permits_investing(access::INVEST));
	}

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
