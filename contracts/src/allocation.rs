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
//! operator ─ IssueUnits ─────────▶ units minted IN KIND to a user or the company
//! operator ─ TransferCompanyStake ▶ the company's units handed to a user, supply unchanged
//! operator ─ RetireUnits ────────▶ a holder's units burnt IN KIND, supply shrinks
//! operator ─ SetAllocationBacking ▶ cash | in_kind — whether Redeem may pay out
//! investor ─ FundsService.Redeem ───────▶ refused while `in_kind` (sell on the book instead)
//! operator ─ ListUnitHolders ────▶ company / fee / investor split of the supply
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
//! `BalanceService`, keyed by the same `service` slug this registry owns. The one
//! exception is `IssueUnits`: supply an operator mints **in kind** — no cash leg — to a
//! [`holder`] that is an investor or the company itself, for a product registered
//! against an asset that already has owners — and `TransferCompanyStake`, the way those
//! units come back out of the company to a named user without the supply moving — and
//! `RetireUnits`, the mint's mirror. Its vocabularies ([`holder`], [`issuance_source`],
//! [`issuance_state`]) are pinned here like the others, as is [`backing`]: whether a
//! product's units have the fund's cash behind them, which is what decides whether
//! `Redeem` may pay them out.

pub use crate::banking::v1::{
	Allocation, AllocationAccessGrant, AllocationAccessGrantList, AllocationList, GetAllocationRequest, GrantAllocationAccessRequest, IssueUnitsRequest, ListAllocationAccessGrantsRequest,
	ListAllocationsRequest, ListUnitHoldersRequest, RegisterAllocationRequest, RetireUnitsRequest, RevokeAllocationAccessRequest, RevokeAllocationAccessResponse, SetAllocationAccessRequest,
	SetAllocationBackingRequest, SetAllocationStateRequest, SetAllocationUnitCapRequest, TransferCompanyStakeRequest, UnitHolders, UnitIssuance, UpdateAllocationRequest,
	allocations_service_client::AllocationsServiceClient,
	allocations_service_server::{AllocationsService, AllocationsServiceServer},
	issue_units_request::Holder as IssueUnitsHolder,
	retire_units_request::Holder as RetireUnitsHolder,
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

/// The canonical `Allocation.backing` strings — what stands behind a product's units.
///
/// The fourth vocabulary of the contract. The hub's `domain::allocations::
/// AllocationBacking` serializes to exactly these (`allocation_backing_strings_are_canonical`
/// and `domain_backing_matches_the_wire_contract` guard the two sides). It gates money on
/// the way OUT: `Redeem` pays cash out of the fund's claim, and a product whose units
/// were minted in kind has none there, so `Redeem` is refused while the backing is
/// [`IN_KIND`] and the holder's exit is the book. A client renders the redeem control off
/// this, never off `state` alone.
pub mod backing {
	/// The units were paid for with cash into the fund's claim; a redemption pays out of
	/// it. What a registration lands on.
	pub const CASH: &str = "cash";
	/// The units stand for an asset held in kind; the fund holds no cash for them.
	/// `Redeem` is refused. Set automatically by the first `IssueUnits`.
	pub const IN_KIND: &str = "in_kind";

	/// Every backing.
	pub const ALL: [&str; 2] = [CASH, IN_KIND];

	/// The backing a product carries from registration until a mint or an operator
	/// changes it.
	pub const DEFAULT: &str = CASH;

	/// Whether `backing` is one this contract defines.
	pub fn is_known(backing: &str) -> bool {
		ALL.contains(&backing)
	}

	/// Whether a product at `backing` lets holders redeem for cash. The authoritative
	/// check runs hub-side on `Redeem`; this is for a client deciding which exit to draw.
	pub fn permits_redemptions(backing: &str) -> bool {
		backing == CASH
	}
}

/// The canonical `UnitIssuance.holder_kind` strings — who an in-kind issuance minted
/// units to.
///
/// The hub's `domain::issuance::UnitHolder` stores exactly these
/// (`unit_holder_strings_are_canonical` guards that side). `company` is a holder in its
/// own right, not a user with a well-known id: it has no `users` row, no position and no
/// P&L, so a client must never try to resolve its (empty) `holder_id` as a user.
pub mod holder {
	/// An investor; `holder_id` is their banking user id.
	pub const USER: &str = "user";
	/// The fund's own stake; `holder_id` is empty.
	pub const COMPANY: &str = "company";

	/// Every holder kind.
	pub const ALL: [&str; 2] = [USER, COMPANY];

	/// Whether `kind` is one this contract defines.
	pub fn is_known(kind: &str) -> bool {
		ALL.contains(&kind)
	}
}

/// The canonical `UnitIssuance.source` strings — where an issuance's units came from.
///
/// The hub's `domain::issuance::IssuanceSource` stores exactly these
/// (`issuance_source_strings_are_canonical` guards that side). A `mint` grew the
/// supply by the row's `units`; a `company` row moved them out of the company's stake
/// and left the supply alone; a `retire` row burnt them and shrank the supply. `units`
/// is always the magnitude — a client summing issuances into "units created" adds the
/// first, ignores the second and subtracts the third.
pub mod issuance_source {
	/// Minted in kind (`IssueUnits`).
	pub const MINT: &str = "mint";
	/// Handed over out of the company's stake (`TransferCompanyStake`).
	pub const COMPANY: &str = "company";
	/// Burnt out of the holder's account (`RetireUnits`).
	pub const RETIRE: &str = "retire";

	/// Every source.
	pub const ALL: [&str; 3] = [MINT, COMPANY, RETIRE];

	/// Whether `source` is one this contract defines.
	pub fn is_known(source: &str) -> bool {
		ALL.contains(&source)
	}
}

/// The canonical `UnitIssuance.state` strings.
///
/// The hub's `domain::issuance::IssuanceState` stores exactly these
/// (`issuance_state_strings_are_canonical` guards that side). Two states only: an
/// issuance is recorded (`queued`) and then minted by the relay (`applied`); there is
/// no failure state because a mint that parks stays `queued` and is surfaced through
/// the parked-event surface, never silently dropped.
pub mod issuance_state {
	/// Recorded; the relay has not posted the mint yet.
	pub const QUEUED: &str = "queued";
	/// The units are on the ledger.
	pub const APPLIED: &str = "applied";

	/// Every state, in lifecycle order.
	pub const ALL: [&str; 2] = [QUEUED, APPLIED];

	/// Whether `state` is one this contract defines.
	pub fn is_known(state: &str) -> bool {
		ALL.contains(&state)
	}

	/// Whether the units are on the ledger. A client waiting to show a holder their
	/// new balance polls until this is true.
	pub fn is_applied(state: &str) -> bool {
		state == APPLIED
	}
}

#[cfg(test)]
mod tests {
	use super::{access, backing, holder, icon, issuance_source, issuance_state, state};

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
	fn the_holder_vocabulary_is_closed_and_canonical() {
		// Byte-identical with `domain::issuance::UnitHolder::kind_str`
		// (`unit_holder_strings_are_canonical` guards the other side).
		assert_eq!(holder::ALL, ["user", "company"]);
		assert!(holder::ALL.iter().all(|h| holder::is_known(h)));
		assert!(!holder::is_known("fund"));
		assert!(!holder::is_known(""));
		assert!(!holder::is_known("Company"), "the wire form is lowercase");
	}

	#[test]
	fn the_issuance_source_vocabulary_is_closed_and_canonical() {
		// Byte-identical with `domain::issuance::IssuanceSource::as_str`
		// (`issuance_source_strings_are_canonical` guards the other side).
		assert_eq!(issuance_source::ALL, ["mint", "company", "retire"]);
		assert!(issuance_source::ALL.iter().all(|s| issuance_source::is_known(s)));
		assert!(!issuance_source::is_known("transfer"));
		assert!(!issuance_source::is_known("burn"));
		assert!(!issuance_source::is_known(""));
		assert!(!issuance_source::is_known("Company"), "the wire form is lowercase");
	}

	#[test]
	fn the_issuance_state_vocabulary_is_closed_and_canonical() {
		// Byte-identical with `domain::issuance::IssuanceState::as_str`
		// (`issuance_state_strings_are_canonical` guards the other side).
		assert_eq!(issuance_state::ALL, ["queued", "applied"]);
		assert!(issuance_state::ALL.iter().all(|s| issuance_state::is_known(s)));
		assert!(!issuance_state::is_known("minted"));
		assert!(issuance_state::is_applied(issuance_state::APPLIED));
		assert!(!issuance_state::is_applied(issuance_state::QUEUED));
	}

	#[test]
	fn the_backing_vocabulary_is_closed_canonical_and_gates_the_exit() {
		// Byte-identical with `domain::allocations::AllocationBacking::as_str`
		// (`allocation_backing_strings_are_canonical` guards the other side).
		assert_eq!(backing::ALL, ["cash", "in_kind"]);
		assert!(backing::ALL.iter().all(|b| backing::is_known(b)));
		assert!(!backing::is_known("asset"));
		assert!(!backing::is_known(""));
		assert!(!backing::is_known("InKind"), "the wire form is lowercase snake_case");
		// A registration is cash-backed, and that is the only backing a redemption pays on.
		assert_eq!(backing::DEFAULT, backing::CASH);
		assert!(backing::permits_redemptions(backing::CASH));
		assert!(!backing::permits_redemptions(backing::IN_KIND));
	}

	#[test]
	fn the_default_icon_is_a_member_of_the_set() {
		// The hub lands an un-iconed registration here, so a client that cannot draw it
		// would leave every fresh product blank.
		assert_eq!(icon::DEFAULT, icon::FUND);
		assert!(icon::is_known(icon::DEFAULT));
	}
}
