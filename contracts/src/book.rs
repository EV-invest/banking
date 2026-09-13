//! The **Book** contract — the curated entry point for every conversation about the
//! secondary market in an allocation's units.
//!
//! A consumer imports this one module instead of hunting through the generated
//! `banking::v1` namespace: it re-exports the book stubs and message types, and pins the
//! wire vocabularies the two sides must agree on ([`side`], [`kind`], [`tif`],
//! [`state`], [`resolution`]).
//!
//! # The conversation
//!
//! ```text
//! operator ─ SetBookPolicy ──────▶ book_open, taker fee, tick, lot, slippage
//! investor ─ GetBook / WatchBook ▶ the aggregated book, the tape, NAV for the ticker
//! investor ─ PlaceOrder ─────────▶ escrow locked; fills settle DvP through the relay
//! investor ─ ListOpenOrders ─────▶ what is resting (refetch when `orders_revision` moves)
//! investor ─ CancelOrder ────────▶ the unspent escrow comes back
//! investor ─ ListUserTrades ─────▶ own fills, with the fee paid as taker
//! ```
//!
//! The book deals only with holders an operator let in (`invest` on the allocation),
//! and only while the policy says `book_open`. It never mints or burns: every unit that
//! changes hands already existed, so the supply and the NAV are untouched by a trade.

pub use crate::banking::v1::{
	BookEvent, BookLevel, BookPolicy, BookSnapshot, CancelOrderRequest, Candle, CandleList, GetBookPolicyRequest, GetBookRequest, ListCandlesRequest, ListOpenOrdersRequest,
	ListOrderHistoryRequest, ListTradesRequest, ListUserTradesRequest, Order, OrderList, PlaceOrderRequest, SetBookPolicyRequest, Trade, TradeList, WatchBookRequest,
	book_service_client::BookServiceClient,
	book_service_server::{BookService, BookServiceServer},
};

/// The canonical `Order.side` / `Trade.taker_side` / `Trade.user_side` strings.
pub mod side {
	pub const BUY: &str = "buy";
	pub const SELL: &str = "sell";

	pub const ALL: [&str; 2] = [BUY, SELL];

	pub fn is_known(side: &str) -> bool {
		ALL.contains(&side)
	}
}

/// The canonical `Order.kind` strings.
pub mod kind {
	pub const LIMIT: &str = "limit";
	/// Priced by the hub from the best opposite quote ± slippage, then run as an IOC limit.
	pub const MARKET: &str = "market";

	pub const ALL: [&str; 2] = [LIMIT, MARKET];

	pub fn is_known(kind: &str) -> bool {
		ALL.contains(&kind)
	}
}

/// The canonical `Order.tif` strings.
pub mod tif {
	/// Good till cancelled.
	pub const GTC: &str = "gtc";
	/// Immediate or cancel.
	pub const IOC: &str = "ioc";
	/// Add liquidity only (post-only).
	pub const ALO: &str = "alo";

	pub const ALL: [&str; 3] = [GTC, IOC, ALO];

	pub fn is_known(tif: &str) -> bool {
		ALL.contains(&tif)
	}
}

/// The canonical `Order.state` strings.
///
/// The hub's `domain::book::OrderState` stores exactly these (`book_strings_are_canonical`
/// guards that side). `open` and `partially_filled` are the two RESTING states — an order
/// in either sits on the book and can be cancelled; the other three are terminal.
pub mod state {
	pub const OPEN: &str = "open";
	pub const PARTIALLY_FILLED: &str = "partially_filled";
	pub const FILLED: &str = "filled";
	pub const CANCELLED: &str = "cancelled";
	/// The ledger refused the order's escrow after it was recorded; `reject_reason` says why.
	pub const REJECTED: &str = "rejected";

	/// Every state, in lifecycle order.
	pub const ALL: [&str; 5] = [OPEN, PARTIALLY_FILLED, FILLED, CANCELLED, REJECTED];

	pub fn is_known(state: &str) -> bool {
		ALL.contains(&state)
	}

	/// Whether an order in `state` sits on the book (and may be cancelled).
	pub fn is_resting(state: &str) -> bool {
		state == OPEN || state == PARTIALLY_FILLED
	}
}

/// The canonical `ListCandlesRequest.resolution` strings.
pub mod resolution {
	pub const M1: &str = "1m";
	pub const M5: &str = "5m";
	pub const M15: &str = "15m";
	pub const H1: &str = "1h";
	pub const H4: &str = "4h";
	pub const D1: &str = "1d";

	/// Every resolution, narrowest first.
	pub const ALL: [&str; 6] = [M1, M5, M15, H1, H4, D1];

	pub fn is_known(resolution: &str) -> bool {
		ALL.contains(&resolution)
	}
}

#[cfg(test)]
mod tests {
	use super::{kind, resolution, side, state, tif};

	#[test]
	fn the_book_vocabularies_are_closed_and_canonical() {
		// Byte-identical with `domain::book` (`book_strings_are_canonical` guards the other
		// side); a consumer matches on these constants rather than its own literals.
		assert_eq!(side::ALL, ["buy", "sell"]);
		assert_eq!(kind::ALL, ["limit", "market"]);
		assert_eq!(tif::ALL, ["gtc", "ioc", "alo"]);
		assert_eq!(state::ALL, ["open", "partially_filled", "filled", "cancelled", "rejected"]);
		assert_eq!(resolution::ALL, ["1m", "5m", "15m", "1h", "4h", "1d"]);
		assert!(side::ALL.iter().all(|s| side::is_known(s)));
		assert!(kind::ALL.iter().all(|k| kind::is_known(k)));
		assert!(tif::ALL.iter().all(|t| tif::is_known(t)));
		assert!(state::ALL.iter().all(|s| state::is_known(s)));
		assert!(resolution::ALL.iter().all(|r| resolution::is_known(r)));
		assert!(!side::is_known("long"));
		assert!(!tif::is_known("fok"));
		assert!(!state::is_known("Open"), "the wire form is lowercase");
		assert!(!resolution::is_known("2h"));
	}

	#[test]
	fn only_the_two_resting_states_can_be_cancelled() {
		assert!(state::is_resting(state::OPEN));
		assert!(state::is_resting(state::PARTIALLY_FILLED));
		assert!(!state::is_resting(state::FILLED));
		assert!(!state::is_resting(state::CANCELLED));
		assert!(!state::is_resting(state::REJECTED));
	}
}
