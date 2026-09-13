//! `book` bounded context — the secondary market in a fund's units.
//!
//! A [`Subscription`](crate::subscriptions::Subscription) buys units from the fund at
//! NAV and a redemption sells them back to it; both are dealings *with the fund*. The
//! book is where holders deal **with each other**: a central limit order book per
//! allocation, price-time priority, in the shape of a spot exchange. NAV stays the
//! accounting price — positions, P&L and the fee high-water mark are still measured at
//! it — and the book's last trade is a second, market quote beside it, never a
//! replacement.
//!
//! Money-wise a resting order is an **escrow**: a sell moves its units from the holder's
//! `UserShares` into `BookShares`, a buy moves its worst-case cash (`size × price`, plus
//! the taker fee it might owe) from `UserClaim` into `BookCash`. A fill is one linked
//! TigerBeetle batch — units out of the seller's escrow into the buyer's holding, cash out
//! of the buyer's escrow into the seller's claim, the taker's fee into `FeeRevenue` —
//! delivery versus payment, all or nothing. Whatever the escrow still holds when the
//! order reaches a terminal state is released back. Those facts are the [`BookEvent`]s
//! the relay posts; this module only decides them.
//!
//! The matching itself is a **pure function behind a trait** ([`MatchingEngine`]): it
//! takes the incoming order and the opposite side of the book (best first) and answers
//! with fills, a remainder and whether the remainder rests — or a rejection. The
//! persistence adapter runs it under the book's write lock and records the answer; the
//! engine never sees a database, a clock or a ledger, which is what makes it exhaustively
//! unit-testable here and replaceable later without touching the money path.
//!
//! Pure and wasm-safe: ids are minted by the application layer, no clock, no I/O.

use ev::architecture::{DomainEvent, Id};
use serde::{Deserialize, Serialize};

use crate::{
	balance::ServiceId,
	error::DomainError,
	money::{Nav, SCALE, Shares, Usdt, mul_div_floor},
	users::UserId,
};

/// A unique order id (UUID). Minted by the application layer.
pub type OrderId = Id<OrderTag>;
/// Phantom tag making [`OrderId`] a distinct, incompatible identity type.
pub struct OrderTag;

/// A unique trade id (UUID). Minted by the persistence adapter as the fill is recorded.
pub type TradeId = Id<TradeTag>;
/// Phantom tag making [`TradeId`] a distinct, incompatible identity type.
pub struct TradeTag;

/// The longest accepted client order id.
pub const MAX_CLIENT_ORDER_ID_LEN: usize = 64;

/// One hundred percent, in basis points — the denominator of every `_bps` field.
pub const BPS_DENOMINATOR: u128 = 10_000;

/// The caller's own idempotency key for one order, unique per user. Trimmed, 1..=64
/// chars — the same shape as an issuance's key, for the same reason: a client retrying
/// a timed-out `PlaceOrder` must land one order, never two.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ClientOrderId(String);

impl ClientOrderId {
	pub fn parse(raw: &str) -> Result<Self, DomainError> {
		let value = raw.trim();
		if value.is_empty() || value.chars().count() > MAX_CLIENT_ORDER_ID_LEN {
			return Err(DomainError::Validation(format!("client order id must be 1..{MAX_CLIENT_ORDER_ID_LEN} chars")));
		}
		Ok(Self(value.to_owned()))
	}

	pub fn as_str(&self) -> &str {
		&self.0
	}
}

/// Which way an order deals.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
	Buy,
	Sell,
}

impl Side {
	/// The stored/wire discriminant. Keep byte-identical with
	/// `evbanking_contracts::book::side` (`book_strings_are_canonical` guards this side).
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Buy => "buy",
			Self::Sell => "sell",
		}
	}

	pub fn parse(raw: &str) -> Result<Self, DomainError> {
		match raw {
			"buy" => Ok(Self::Buy),
			"sell" => Ok(Self::Sell),
			other => Err(DomainError::Validation(format!("unknown order side: {other}"))),
		}
	}

	pub fn opposite(self) -> Self {
		match self {
			Self::Buy => Self::Sell,
			Self::Sell => Self::Buy,
		}
	}
}

/// Whether the caller named a price or asked for the best available one.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderKind {
	Limit,
	/// Priced by the hub from the best opposite quote ± the policy's slippage and then
	/// treated exactly as an IOC limit — so a market order can never fill further from
	/// the quote the caller saw than the policy allows, and never rests.
	Market,
}

impl OrderKind {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Limit => "limit",
			Self::Market => "market",
		}
	}

	pub fn parse(raw: &str) -> Result<Self, DomainError> {
		match raw {
			"limit" => Ok(Self::Limit),
			"market" => Ok(Self::Market),
			other => Err(DomainError::Validation(format!("unknown order kind: {other}"))),
		}
	}
}

/// How long an order may live.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tif {
	/// Good till cancelled: whatever does not fill on arrival rests.
	Gtc,
	/// Immediate or cancel: whatever does not fill on arrival is released.
	Ioc,
	/// Add liquidity only (post-only): rests in full, or is refused if any part of it
	/// would have taken.
	Alo,
}

impl Tif {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Gtc => "gtc",
			Self::Ioc => "ioc",
			Self::Alo => "alo",
		}
	}

	pub fn parse(raw: &str) -> Result<Self, DomainError> {
		match raw {
			"gtc" => Ok(Self::Gtc),
			"ioc" => Ok(Self::Ioc),
			"alo" => Ok(Self::Alo),
			other => Err(DomainError::Validation(format!("unknown time in force: {other}"))),
		}
	}
}

/// Where an order stands. `Open` and `PartiallyFilled` are the two *resting* states —
/// the ones the matcher reads — and every other state is terminal.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderState {
	Open,
	PartiallyFilled,
	Filled,
	Cancelled,
	/// The ledger refused the order's escrow after it was recorded (a raced over-spend the
	/// optimistic Read-First could not see). Written by the relay, never by a caller.
	Rejected,
}

impl OrderState {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Open => "open",
			Self::PartiallyFilled => "partially_filled",
			Self::Filled => "filled",
			Self::Cancelled => "cancelled",
			Self::Rejected => "rejected",
		}
	}

	pub fn parse(raw: &str) -> Result<Self, DomainError> {
		match raw {
			"open" => Ok(Self::Open),
			"partially_filled" => Ok(Self::PartiallyFilled),
			"filled" => Ok(Self::Filled),
			"cancelled" => Ok(Self::Cancelled),
			"rejected" => Ok(Self::Rejected),
			other => Err(DomainError::Validation(format!("unknown order state: {other}"))),
		}
	}

	/// Whether an order in this state sits on the book.
	pub fn is_resting(self) -> bool {
		matches!(self, Self::Open | Self::PartiallyFilled)
	}
}

/// The bucket width of a candle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CandleResolution {
	M1,
	M5,
	M15,
	H1,
	H4,
	D1,
}

impl CandleResolution {
	pub const ALL: [Self; 6] = [Self::M1, Self::M5, Self::M15, Self::H1, Self::H4, Self::D1];

	pub fn as_str(self) -> &'static str {
		match self {
			Self::M1 => "1m",
			Self::M5 => "5m",
			Self::M15 => "15m",
			Self::H1 => "1h",
			Self::H4 => "4h",
			Self::D1 => "1d",
		}
	}

	pub fn parse(raw: &str) -> Result<Self, DomainError> {
		match raw {
			"1m" => Ok(Self::M1),
			"5m" => Ok(Self::M5),
			"15m" => Ok(Self::M15),
			"1h" => Ok(Self::H1),
			"4h" => Ok(Self::H4),
			"1d" => Ok(Self::D1),
			other => Err(DomainError::Validation(format!("unknown candle resolution: {other}"))),
		}
	}

	pub const fn seconds(self) -> i64 {
		match self {
			Self::M1 => 60,
			Self::M5 => 5 * 60,
			Self::M15 => 15 * 60,
			Self::H1 => 60 * 60,
			Self::H4 => 4 * 60 * 60,
			Self::D1 => 24 * 60 * 60,
		}
	}
}

/// A book price — USDT per unit, 18-dp fixed point like [`Nav`], but its own type: a
/// quote two holders agreed on is not a valuation an operator posted, and the two must
/// never be mixed in a position's arithmetic.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Price(u128);

impl Price {
	pub const fn from_base_units(units: u128) -> Self {
		Self(units)
	}

	pub const fn base_units(self) -> u128 {
		self.0
	}

	pub const fn is_zero(self) -> bool {
		self.0 == 0
	}

	pub fn parse_decimal(raw: &str) -> Result<Self, DomainError> {
		Usdt::parse_decimal(raw).map(|usdt| Self(usdt.base_units()))
	}

	pub fn to_decimal_string(self) -> String {
		Usdt::from_base_units(self.0).to_decimal_string()
	}

	/// The cash value of `units` at this price: `floor(units · price / 10^18)`. Floored
	/// like every valuation here — the sub-unit residual stays with whoever pays.
	pub fn value(self, units: Shares) -> Result<Usdt, DomainError> {
		mul_div_floor(units.base_units(), self.0, SCALE)
			.map(Usdt::from_base_units)
			.ok_or_else(|| DomainError::Validation("trade value overflows".into()))
	}

	/// The average price `notional` bought `units` at: `floor(notional · 10^18 / units)`.
	/// `None` for an order that has not filled anything yet.
	pub fn average(notional: Usdt, units: Shares) -> Option<Self> {
		if units.is_zero() {
			return None;
		}
		mul_div_floor(notional.base_units(), SCALE, units.base_units()).map(Self)
	}

	/// `floor(self · numer / denom)` — how a market order's limit is derived from a quote.
	pub fn scale(self, numer: u128, denom: u128) -> Result<Self, DomainError> {
		mul_div_floor(self.0, numer, denom)
			.map(Self)
			.ok_or_else(|| DomainError::Validation("price scaling overflows or divides by zero".into()))
	}

	/// Whether this price sits on the policy's tick grid.
	pub fn is_aligned(self, tick: Price) -> bool {
		!tick.is_zero() && self.0.is_multiple_of(tick.0)
	}

	/// The nearest tick at or below this price.
	pub fn floor_to(self, tick: Price) -> Self {
		if tick.is_zero() {
			return self;
		}
		Self(self.0 - self.0 % tick.0)
	}

	/// The nearest tick at or above this price (saturating at the top of the range).
	pub fn ceil_to(self, tick: Price) -> Self {
		if tick.is_zero() || self.0.is_multiple_of(tick.0) {
			return self;
		}
		Self(self.floor_to(tick).0.saturating_add(tick.0))
	}
}

impl Serialize for Price {
	fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
		serializer.serialize_str(&self.0.to_string())
	}
}

impl<'de> Deserialize<'de> for Price {
	fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		let raw = String::deserialize(deserializer)?;
		raw.parse::<u128>().map(Self).map_err(serde::de::Error::custom)
	}
}

impl core::fmt::Display for Price {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		f.write_str(&self.to_decimal_string())
	}
}

/// An order size that has been checked against the policy's lot: positive and a whole
/// number of lots. Construct through [`BookPolicy::size`]; the inner [`Shares`] is what
/// the rest of the context deals in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OrderSize(Shares);

impl OrderSize {
	pub const fn shares(self) -> Shares {
		self.0
	}
}

/// One allocation's trading terms. Set by an `AllocationManage` holder; a product with no
/// policy row trades under [`BookPolicy::default`], whose `book_open` is `false` — the
/// book is opt-in per product, exactly as the fee is.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BookPolicy {
	book_open: bool,
	taker_fee_bps: u32,
	price_tick: Price,
	lot_size: Shares,
	market_slippage_bps: u32,
}

impl BookPolicy {
	/// `0.0001` units — the lot a product trades in until an operator says otherwise.
	pub const DEFAULT_LOT_SIZE: Shares = Shares::from_base_units(SCALE / 10_000);
	/// 5 % — how far past the best quote a market order may fill.
	pub const DEFAULT_MARKET_SLIPPAGE_BPS: u32 = 500;
	/// `0.01` USDT — the tick a product trades on until an operator says otherwise.
	pub const DEFAULT_PRICE_TICK: Price = Price::from_base_units(SCALE / 100);

	/// Validate a full set of terms. Every bps figure is at most one hundred percent;
	/// tick and lot are positive, because a zero grid admits every price and every size
	/// and would make "aligned" meaningless.
	pub fn new(book_open: bool, taker_fee_bps: u32, price_tick: Price, lot_size: Shares, market_slippage_bps: u32) -> Result<Self, DomainError> {
		if u128::from(taker_fee_bps) > BPS_DENOMINATOR {
			return Err(DomainError::Validation(format!("taker fee must be at most {BPS_DENOMINATOR} bps")));
		}
		if u128::from(market_slippage_bps) > BPS_DENOMINATOR {
			return Err(DomainError::Validation(format!("market slippage must be at most {BPS_DENOMINATOR} bps")));
		}
		if price_tick.is_zero() {
			return Err(DomainError::Validation("price tick must be positive".into()));
		}
		if lot_size.is_zero() {
			return Err(DomainError::Validation("lot size must be positive".into()));
		}
		Ok(Self {
			book_open,
			taker_fee_bps,
			price_tick,
			lot_size,
			market_slippage_bps,
		})
	}

	pub fn book_open(&self) -> bool {
		self.book_open
	}

	pub fn taker_fee_bps(&self) -> u32 {
		self.taker_fee_bps
	}

	pub fn price_tick(&self) -> Price {
		self.price_tick
	}

	pub fn lot_size(&self) -> Shares {
		self.lot_size
	}

	pub fn market_slippage_bps(&self) -> u32 {
		self.market_slippage_bps
	}

	/// A limit price the caller named, checked against the grid.
	pub fn price(&self, price: Price) -> Result<Price, DomainError> {
		if price.is_zero() {
			return Err(DomainError::Validation("price must be positive".into()));
		}
		if !price.is_aligned(self.price_tick) {
			return Err(DomainError::Validation(format!("price must be a multiple of the {} tick", self.price_tick)));
		}
		Ok(price)
	}

	/// An order size the caller named, checked against the lot.
	pub fn size(&self, size: Shares) -> Result<OrderSize, DomainError> {
		if size.is_zero() {
			return Err(DomainError::Validation("size must be positive".into()));
		}
		if !size.base_units().is_multiple_of(self.lot_size.base_units()) {
			return Err(DomainError::Validation(format!("size must be a multiple of the {} lot", self.lot_size)));
		}
		Ok(OrderSize(size))
	}

	/// What a taker owes on a fill of `notional`: `floor(notional × bps / 10 000)`.
	/// Floored, so the rounding residual stays with the trader, never with the fee.
	pub fn taker_fee(&self, notional: Usdt) -> Result<Usdt, DomainError> {
		notional.scale(u128::from(self.taker_fee_bps), BPS_DENOMINATOR)
	}

	/// The cash a buy order commits on placement: its full notional at the limit plus
	/// the taker fee on it. The worst case on both counts — every fill lands at or below
	/// the limit, and flooring makes the fee on a sum at least the sum of the fees on its
	/// parts — so the escrow always covers what the fills will take, and a maker (who
	/// owes no fee) or a price improvement gets the difference back at the end.
	pub fn buy_reserve(&self, size: Shares, price: Price) -> Result<Usdt, DomainError> {
		let notional = price.value(size)?;
		let fee = self.taker_fee(notional)?;
		notional.checked_add(fee).ok_or_else(|| DomainError::Validation("order reserve overflows".into()))
	}

	/// The limit a market order is given from the best opposite quote: `best × (1 +
	/// slippage)` rounded **up** to the tick for a buy, `best × (1 − slippage)` rounded
	/// **down** for a sell — the rounding always widens, so the caller is never refused a
	/// fill the policy meant to allow. A sell whose limit rounds to zero is refused
	/// rather than sold for nothing.
	pub fn market_limit(&self, side: Side, best_opposite: Price) -> Result<Price, DomainError> {
		let slippage = u128::from(self.market_slippage_bps);
		let limit = match side {
			Side::Buy => best_opposite.scale(BPS_DENOMINATOR + slippage, BPS_DENOMINATOR)?.ceil_to(self.price_tick),
			Side::Sell => best_opposite.scale(BPS_DENOMINATOR - slippage, BPS_DENOMINATOR)?.floor_to(self.price_tick),
		};
		if limit.is_zero() {
			return Err(DomainError::Validation("market sell would fill at a zero price".into()));
		}
		Ok(limit)
	}
}

impl Default for BookPolicy {
	fn default() -> Self {
		Self {
			book_open: false,
			taker_fee_bps: 0,
			price_tick: Self::DEFAULT_PRICE_TICK,
			lot_size: Self::DEFAULT_LOT_SIZE,
			market_slippage_bps: Self::DEFAULT_MARKET_SLIPPAGE_BPS,
		}
	}
}

/// What an order holds in escrow: units for a sell, cash for a buy. Tagged so the stored
/// event payload is self-describing.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "amount", rename_all = "snake_case")]
pub enum Locked {
	Units(Shares),
	Cash(Usdt),
}

impl Locked {
	pub fn is_zero(self) -> bool {
		match self {
			Self::Units(units) => units.is_zero(),
			Self::Cash(cash) => cash.is_zero(),
		}
	}
}

/// The stored shape of an [`Order`], as the persistence adapter reads it back — the
/// input to [`Order::rehydrate`].
pub struct OrderSnapshot {
	pub id: OrderId,
	pub service: ServiceId,
	pub user: UserId,
	pub client_order_id: ClientOrderId,
	pub side: Side,
	pub kind: OrderKind,
	pub tif: Tif,
	pub price: Price,
	pub size: Shares,
	pub filled: Shares,
	pub notional_filled: Usdt,
	pub fee_paid: Usdt,
	pub reserved: Locked,
	pub state: OrderState,
	pub reject_reason: Option<String>,
}

/// One order — placed, filled in steps, and finally filled, cancelled or rejected. It
/// raises no events of its own: the facts the relay needs are decided by the placement
/// as a whole (see [`BookEvent`]), which spans this order, the makers it hit and the
/// trades between them.
#[derive(Clone, Debug)]
pub struct Order {
	id: OrderId,
	service: ServiceId,
	user: UserId,
	client_order_id: ClientOrderId,
	side: Side,
	kind: OrderKind,
	tif: Tif,
	price: Price,
	size: Shares,
	filled: Shares,
	notional_filled: Usdt,
	fee_paid: Usdt,
	reserved: Locked,
	state: OrderState,
	reject_reason: Option<String>,
}

impl Order {
	/// A fresh order, already priced (a market order carries the limit the hub derived
	/// for it) and sized on the grid, with the escrow it commits computed by the caller
	/// from the policy. Starts `Open`.
	// Ten arguments because an order IS these ten facts: bundling them into a request
	// struct would restate the field list once more for no second caller to share it.
	#[allow(clippy::too_many_arguments)]
	pub fn place(
		id: OrderId,
		service: ServiceId,
		user: UserId,
		client_order_id: ClientOrderId,
		side: Side,
		kind: OrderKind,
		tif: Tif,
		price: Price,
		size: OrderSize,
		reserved: Locked,
	) -> Self {
		Self {
			id,
			service,
			user,
			client_order_id,
			side,
			kind,
			tif,
			price,
			size: size.shares(),
			filled: Shares::ZERO,
			notional_filled: Usdt::ZERO,
			fee_paid: Usdt::ZERO,
			reserved,
			state: OrderState::Open,
			reject_reason: None,
		}
	}

	/// Reconstitute from the store.
	pub fn rehydrate(snapshot: OrderSnapshot) -> Self {
		Self {
			id: snapshot.id,
			service: snapshot.service,
			user: snapshot.user,
			client_order_id: snapshot.client_order_id,
			side: snapshot.side,
			kind: snapshot.kind,
			tif: snapshot.tif,
			price: snapshot.price,
			size: snapshot.size,
			filled: snapshot.filled,
			notional_filled: snapshot.notional_filled,
			fee_paid: snapshot.fee_paid,
			reserved: snapshot.reserved,
			state: snapshot.state,
			reject_reason: snapshot.reject_reason,
		}
	}

	/// Whether a retry under the same client order id is asking for the same thing.
	pub fn matches_request(&self, service: &ServiceId, side: Side, kind: OrderKind, tif: Tif, size: Shares) -> bool {
		&self.service == service && self.side == side && self.kind == kind && self.tif == tif && self.size == size
	}

	/// The engine's view of this order while it rests.
	pub fn resting(&self) -> RestingOrder {
		RestingOrder {
			id: self.id,
			user: self.user,
			price: self.price,
			remaining: self.remaining(),
		}
	}

	/// Record `size` units traded at `price`, charging `fee` to this order. Moves to
	/// `PartiallyFilled` or `Filled`. Refuses more than is left, and refuses an order
	/// that is not resting — a fill against a cancelled order is a matcher bug, not a
	/// state to record.
	pub fn fill(&mut self, size: Shares, price: Price, fee: Usdt) -> Result<(), DomainError> {
		if !self.state.is_resting() {
			return Err(DomainError::Conflict(format!("order is {}, not fillable", self.state.as_str())));
		}
		if size.is_zero() || size > self.remaining() {
			return Err(DomainError::Validation("fill exceeds the order's remaining size".into()));
		}
		let notional = price.value(size)?;
		self.filled = self.filled.checked_add(size).ok_or_else(|| DomainError::Validation("filled size overflows".into()))?;
		self.notional_filled = self
			.notional_filled
			.checked_add(notional)
			.ok_or_else(|| DomainError::Validation("filled notional overflows".into()))?;
		self.fee_paid = self.fee_paid.checked_add(fee).ok_or_else(|| DomainError::Validation("fee paid overflows".into()))?;
		self.state = if self.filled == self.size { OrderState::Filled } else { OrderState::PartiallyFilled };
		Ok(())
	}

	/// Take the order off the book (the caller's cancel, or an IOC remainder). Idempotent
	/// on an already-cancelled order; a filled or rejected one is a `Conflict`.
	pub fn cancel(&mut self) -> Result<(), DomainError> {
		match self.state {
			OrderState::Cancelled => Ok(()),
			OrderState::Open | OrderState::PartiallyFilled => {
				self.state = OrderState::Cancelled;
				Ok(())
			}
			OrderState::Filled | OrderState::Rejected => Err(DomainError::Conflict(format!("order is {}, not cancellable", self.state.as_str()))),
		}
	}

	/// What the escrow still holds once the order is terminal — units not sold, or cash
	/// not spent on fills and fees. `None` while the order rests (nothing is released
	/// early) or when nothing is left.
	pub fn release(&self) -> Option<Locked> {
		if self.state.is_resting() || self.state == OrderState::Rejected {
			return None;
		}
		let left = match self.reserved {
			Locked::Units(units) => Locked::Units(units.checked_sub(self.filled)?),
			Locked::Cash(cash) => Locked::Cash(cash.checked_sub(self.notional_filled)?.checked_sub(self.fee_paid)?),
		};
		(!left.is_zero()).then_some(left)
	}

	pub fn id(&self) -> OrderId {
		self.id
	}

	pub fn service(&self) -> &ServiceId {
		&self.service
	}

	pub fn user(&self) -> UserId {
		self.user
	}

	pub fn client_order_id(&self) -> &ClientOrderId {
		&self.client_order_id
	}

	pub fn side(&self) -> Side {
		self.side
	}

	pub fn kind(&self) -> OrderKind {
		self.kind
	}

	pub fn tif(&self) -> Tif {
		self.tif
	}

	pub fn price(&self) -> Price {
		self.price
	}

	pub fn size(&self) -> Shares {
		self.size
	}

	pub fn filled(&self) -> Shares {
		self.filled
	}

	pub fn remaining(&self) -> Shares {
		self.size.checked_sub(self.filled).unwrap_or(Shares::ZERO)
	}

	pub fn notional_filled(&self) -> Usdt {
		self.notional_filled
	}

	pub fn fee_paid(&self) -> Usdt {
		self.fee_paid
	}

	pub fn average_fill_price(&self) -> Option<Price> {
		Price::average(self.notional_filled, self.filled)
	}

	pub fn reserved(&self) -> Locked {
		self.reserved
	}

	pub fn state(&self) -> OrderState {
		self.state
	}

	pub fn reject_reason(&self) -> Option<&str> {
		self.reject_reason.as_deref()
	}
}

/// A resting order as the engine sees it. The adapter hands the engine the opposite side
/// **best first** — highest bid first for a sell, lowest ask first for a buy, earliest
/// first within a price — and the engine relies on that order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestingOrder {
	pub id: OrderId,
	pub user: UserId,
	pub price: Price,
	pub remaining: Shares,
}

/// The order arriving at the book: priced (a market order already carries its derived
/// limit), sized and owned.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IncomingOrder {
	pub id: OrderId,
	pub user: UserId,
	pub side: Side,
	pub tif: Tif,
	pub price: Price,
	pub size: Shares,
}

/// One trade the engine decided: `size` units at the **maker's** price (price-time
/// priority — the resting order set the price, the taker crossed it).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Fill {
	pub maker: OrderId,
	pub maker_user: UserId,
	pub price: Price,
	pub size: Shares,
}

/// Why an incoming order was refused as a whole. A rejection fills nothing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Rejection {
	/// The order would have traded against one of the same user's own resting orders.
	SelfTrade,
	/// A post-only order would have taken liquidity.
	WouldCross,
}

impl Rejection {
	pub fn reason(self) -> &'static str {
		match self {
			Self::SelfTrade => "order would trade against your own resting order",
			Self::WouldCross => "post-only order would cross the book",
		}
	}
}

/// What the engine decided for one incoming order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatchOutcome {
	/// In the order they happened; the adapter records a trade for each.
	pub fills: Vec<Fill>,
	/// Units left after the fills.
	pub remainder: Shares,
	/// Whether the remainder stays on the book (GTC/ALO) or is released (IOC, market).
	pub rests: bool,
}

/// The matching seam. Pure: no clock, no I/O, no ledger — the adapter runs it under the
/// book's write lock and persists what it returns. Replace the implementation to change
/// how the book matches; nothing about escrow, settlement or persistence moves with it.
pub trait MatchingEngine: Send + Sync {
	/// Match `incoming` against `resting`, the opposite side best first. `policy` is
	/// available for engines that price or size differently; [`PriceTimeEngine`] has
	/// already had both checked upstream and ignores it.
	fn match_order(&self, incoming: &IncomingOrder, resting: &[RestingOrder], policy: &BookPolicy) -> Result<MatchOutcome, Rejection>;
}

/// Strict price-time priority: walk the opposite side best first, fill at each maker's
/// price while it crosses the incoming limit, stop at the first that does not.
#[derive(Clone, Copy, Debug, Default)]
pub struct PriceTimeEngine;

impl MatchingEngine for PriceTimeEngine {
	fn match_order(&self, incoming: &IncomingOrder, resting: &[RestingOrder], _policy: &BookPolicy) -> Result<MatchOutcome, Rejection> {
		let crosses = |maker: &RestingOrder| match incoming.side {
			Side::Buy => maker.price <= incoming.price,
			Side::Sell => maker.price >= incoming.price,
		};
		let mut remaining = incoming.size;
		let mut fills = Vec::new();
		for maker in resting {
			if remaining.is_zero() || !crosses(maker) {
				break;
			}
			// Refusing the whole order rather than skipping the own order: skipping would
			// let a trader jump their own queue position, and refusing is what every
			// venue's default self-trade policy does.
			if maker.user == incoming.user {
				return Err(Rejection::SelfTrade);
			}
			if incoming.tif == Tif::Alo {
				return Err(Rejection::WouldCross);
			}
			if maker.remaining.is_zero() {
				continue;
			}
			let size = remaining.min(maker.remaining);
			fills.push(Fill {
				maker: maker.id,
				maker_user: maker.user,
				price: maker.price,
				size,
			});
			remaining = remaining.checked_sub(size).unwrap_or(Shares::ZERO);
		}
		let rests = !remaining.is_zero()
			&& match incoming.tif {
				Tif::Gtc | Tif::Alo => true,
				Tif::Ioc => false,
			};
		Ok(MatchOutcome { fills, remainder: remaining, rests })
	}
}

/// One trade as recorded: the two orders, the two parties, the price the maker set, and
/// what the taker paid on top.
#[derive(Clone, Debug)]
pub struct Trade {
	pub id: TradeId,
	pub service: ServiceId,
	pub buyer: UserId,
	pub seller: UserId,
	pub buy_order: OrderId,
	pub sell_order: OrderId,
	pub taker_side: Side,
	pub price: Price,
	pub size: Shares,
	pub notional: Usdt,
	pub fee: Usdt,
}

/// The ledger facts one placement or cancel leaves behind. Standalone facts rather than
/// an aggregate's drained events: a single placement writes the taker's order, every
/// maker it hit and a trade between each pair, and the relay must see them in exactly
/// the order they are listed here — lock the taker, settle each fill, release whatever
/// reached a terminal state — so the adapter writes them itself, in that order, in the
/// placement's transaction. Internally tagged so the stored JSON is self-describing.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BookEvent {
	/// An order was recorded; the relay moves its escrow (`Dr BookShares / Cr UserShares`
	/// for a sell, `Dr UserClaim / Cr BookCash` for a buy).
	OrderPlaced {
		order_id: OrderId,
		service: ServiceId,
		user: UserId,
		side: Side,
		locked: Locked,
	},
	/// Two orders traded. The relay posts one linked batch: `size` units `Dr
	/// UserShares(buyer) / Cr BookShares(seller)`, `notional` cash `Dr BookCash(buyer) /
	/// Cr UserClaim(seller)`, and `fee` from the taker into `FeeRevenue` — then adds the
	/// buyer's cost basis and reduces the seller's. `nav` is the fund's mark at the time,
	/// carried so the buyer's high-water mark blends the accounting price, not the quote.
	TradeExecuted {
		trade_id: TradeId,
		service: ServiceId,
		buyer: UserId,
		seller: UserId,
		buy_order_id: OrderId,
		sell_order_id: OrderId,
		taker_side: Side,
		size: Shares,
		price: Price,
		notional: Usdt,
		fee: Usdt,
		nav: Nav,
	},
	/// An order reached a terminal state with something left in escrow; the relay hands
	/// it back (`Dr UserShares / Cr BookShares` or `Dr BookCash / Cr UserClaim`).
	OrderReleased {
		order_id: OrderId,
		service: ServiceId,
		user: UserId,
		side: Side,
		released: Locked,
	},
}

impl DomainEvent for BookEvent {
	const KIND: &'static str = "book";
}

#[cfg(test)]
mod tests {
	use super::*;

	fn svc() -> ServiceId {
		ServiceId::parse("service_arb").unwrap()
	}

	fn price(raw: &str) -> Price {
		Price::parse_decimal(raw).unwrap()
	}

	fn shares(raw: &str) -> Shares {
		Shares::parse_decimal(raw).unwrap()
	}

	fn usdt(raw: &str) -> Usdt {
		Usdt::parse_decimal(raw).unwrap()
	}

	fn open_policy(fee_bps: u32) -> BookPolicy {
		BookPolicy::new(true, fee_bps, BookPolicy::DEFAULT_PRICE_TICK, BookPolicy::DEFAULT_LOT_SIZE, 500).unwrap()
	}

	fn incoming(user: UserId, side: Side, tif: Tif, at: &str, size: &str) -> IncomingOrder {
		IncomingOrder {
			id: OrderId::new(),
			user,
			side,
			tif,
			price: price(at),
			size: shares(size),
		}
	}

	fn resting(user: UserId, at: &str, remaining: &str) -> RestingOrder {
		RestingOrder {
			id: OrderId::new(),
			user,
			price: price(at),
			remaining: shares(remaining),
		}
	}

	#[test]
	fn book_strings_are_canonical() {
		// Wire contract: these must match `evbanking_contracts::book`.
		assert_eq!(Side::Buy.as_str(), "buy");
		assert_eq!(Side::Sell.as_str(), "sell");
		assert_eq!(OrderKind::Limit.as_str(), "limit");
		assert_eq!(OrderKind::Market.as_str(), "market");
		assert_eq!(Tif::Gtc.as_str(), "gtc");
		assert_eq!(Tif::Ioc.as_str(), "ioc");
		assert_eq!(Tif::Alo.as_str(), "alo");
		for state in [OrderState::Open, OrderState::PartiallyFilled, OrderState::Filled, OrderState::Cancelled, OrderState::Rejected] {
			assert_eq!(OrderState::parse(state.as_str()).unwrap(), state);
			assert_eq!(serde_json::to_string(&state).unwrap(), format!("\"{}\"", state.as_str()));
		}
		for side in [Side::Buy, Side::Sell] {
			assert_eq!(Side::parse(side.as_str()).unwrap(), side);
			assert_eq!(side.opposite().opposite(), side);
		}
		for kind in [OrderKind::Limit, OrderKind::Market] {
			assert_eq!(OrderKind::parse(kind.as_str()).unwrap(), kind);
		}
		for tif in [Tif::Gtc, Tif::Ioc, Tif::Alo] {
			assert_eq!(Tif::parse(tif.as_str()).unwrap(), tif);
		}
		for resolution in CandleResolution::ALL {
			assert_eq!(CandleResolution::parse(resolution.as_str()).unwrap(), resolution);
			assert!(resolution.seconds() > 0);
		}
		assert!(Side::parse("long").is_err());
		assert!(Tif::parse("fok").is_err());
		assert!(OrderState::parse("done").is_err());
		assert!(CandleResolution::parse("2h").is_err());
	}

	#[test]
	fn the_client_order_id_is_trimmed_and_bounded() {
		assert_eq!(ClientOrderId::parse("  c-7 ").unwrap().as_str(), "c-7");
		assert!(ClientOrderId::parse("").is_err());
		assert!(ClientOrderId::parse(&"k".repeat(MAX_CLIENT_ORDER_ID_LEN)).is_ok());
		assert!(ClientOrderId::parse(&"k".repeat(MAX_CLIENT_ORDER_ID_LEN + 1)).is_err());
	}

	#[test]
	fn the_default_policy_is_closed_on_the_documented_grid() {
		let policy = BookPolicy::default();
		assert!(!policy.book_open(), "a product trades only once an operator opens its book");
		assert_eq!(policy.taker_fee_bps(), 0);
		assert_eq!(policy.price_tick().to_decimal_string(), "0.01");
		assert_eq!(policy.lot_size().to_decimal_string(), "0.0001");
		assert_eq!(policy.market_slippage_bps(), 500);
	}

	#[test]
	fn policy_terms_are_bounded() {
		assert!(BookPolicy::new(true, 10_000, price("0.01"), shares("1"), 0).is_ok());
		assert!(BookPolicy::new(true, 10_001, price("0.01"), shares("1"), 0).is_err(), "a fee above 100% is refused");
		assert!(BookPolicy::new(true, 0, price("0.01"), shares("1"), 10_001).is_err(), "slippage above 100% is refused");
		assert!(BookPolicy::new(true, 0, Price::from_base_units(0), shares("1"), 0).is_err(), "a zero tick admits every price");
		assert!(BookPolicy::new(true, 0, price("0.01"), Shares::ZERO, 0).is_err(), "a zero lot admits every size");
	}

	#[test]
	fn prices_and_sizes_must_sit_on_the_grid() {
		let policy = open_policy(0);
		assert_eq!(policy.price(price("1.25")).unwrap(), price("1.25"));
		assert!(policy.price(price("1.255")).is_err(), "off the 0.01 tick");
		assert!(policy.price(Price::from_base_units(0)).is_err(), "a zero price is not a quote");
		assert_eq!(policy.size(shares("2.5")).unwrap().shares(), shares("2.5"));
		assert!(policy.size(shares("0.00005")).is_err(), "off the 0.0001 lot");
		assert!(policy.size(Shares::ZERO).is_err());
	}

	#[test]
	fn a_buy_reserves_notional_plus_the_fee_it_might_owe() {
		// 10 units at 1.50 = 15 USDT; 25 bps of that is 0.0375.
		let policy = open_policy(25);
		assert_eq!(policy.buy_reserve(shares("10"), price("1.5")).unwrap(), usdt("15.0375"));
		assert_eq!(policy.taker_fee(usdt("15")).unwrap(), usdt("0.0375"));
		// The fee floors: 1 base unit at 25 bps is nothing, and stays with the trader.
		assert_eq!(policy.taker_fee(Usdt::from_base_units(1)).unwrap(), Usdt::ZERO);
		// With no fee the reserve is exactly the notional.
		assert_eq!(open_policy(0).buy_reserve(shares("10"), price("1.5")).unwrap(), usdt("15"));
	}

	#[test]
	fn a_market_limit_widens_the_quote_by_the_slippage_and_rounds_outward() {
		// 5 % slippage on a 1.00 quote: a buy may pay up to 1.05, a sell take down to 0.95.
		let policy = open_policy(0);
		assert_eq!(policy.market_limit(Side::Buy, price("1")).unwrap(), price("1.05"));
		assert_eq!(policy.market_limit(Side::Sell, price("1")).unwrap(), price("0.95"));
		// 1.03 × 1.05 = 1.0815 → rounds UP to 1.09 for a buy (never refusing a fill the
		// policy meant to allow); 1.03 × 0.95 = 0.9785 → DOWN to 0.97 for a sell.
		assert_eq!(policy.market_limit(Side::Buy, price("1.03")).unwrap(), price("1.09"));
		assert_eq!(policy.market_limit(Side::Sell, price("1.03")).unwrap(), price("0.97"));
		// Full slippage on a sell rounds to nothing — refused, never sold for zero.
		let all_the_way = BookPolicy::new(true, 0, price("0.01"), shares("1"), 10_000).unwrap();
		assert!(all_the_way.market_limit(Side::Sell, price("1")).is_err());
	}

	#[test]
	fn a_buy_walks_the_asks_best_first_at_the_makers_prices() {
		let (taker, a, b) = (UserId::new(), UserId::new(), UserId::new());
		let asks = [resting(a, "1.00", "5"), resting(b, "1.02", "5"), resting(a, "1.10", "5")];
		let outcome = PriceTimeEngine.match_order(&incoming(taker, Side::Buy, Tif::Gtc, "1.05", "8"), &asks, &open_policy(0)).unwrap();
		// 5 at 1.00, then 3 of the 5 at 1.02; the 1.10 ask is past the limit.
		assert_eq!(outcome.fills.len(), 2);
		assert_eq!((outcome.fills[0].price, outcome.fills[0].size, outcome.fills[0].maker_user), (price("1.00"), shares("5"), a));
		assert_eq!((outcome.fills[1].price, outcome.fills[1].size, outcome.fills[1].maker_user), (price("1.02"), shares("3"), b));
		assert!(outcome.remainder.is_zero());
		assert!(!outcome.rests);
	}

	#[test]
	fn a_sell_walks_the_bids_and_a_gtc_remainder_rests() {
		let (taker, maker) = (UserId::new(), UserId::new());
		let bids = [resting(maker, "1.10", "2"), resting(maker, "1.05", "2"), resting(maker, "0.90", "10")];
		let outcome = PriceTimeEngine.match_order(&incoming(taker, Side::Sell, Tif::Gtc, "1.00", "10"), &bids, &open_policy(0)).unwrap();
		assert_eq!(outcome.fills.len(), 2, "the 0.90 bid is below the sell limit");
		assert_eq!(outcome.remainder, shares("6"));
		assert!(outcome.rests, "GTC: what did not fill goes on the book");
	}

	#[test]
	fn an_ioc_remainder_is_released_not_rested() {
		let (taker, maker) = (UserId::new(), UserId::new());
		let asks = [resting(maker, "1.00", "3")];
		let outcome = PriceTimeEngine.match_order(&incoming(taker, Side::Buy, Tif::Ioc, "1.00", "10"), &asks, &open_policy(0)).unwrap();
		assert_eq!(outcome.fills.len(), 1);
		assert_eq!(outcome.remainder, shares("7"));
		assert!(!outcome.rests);
		// And an IOC that finds nothing fills nothing and rests nothing.
		let nothing = PriceTimeEngine.match_order(&incoming(taker, Side::Buy, Tif::Ioc, "1.00", "10"), &[], &open_policy(0)).unwrap();
		assert!(nothing.fills.is_empty());
		assert_eq!(nothing.remainder, shares("10"));
		assert!(!nothing.rests);
	}

	#[test]
	fn a_post_only_order_rests_in_full_or_is_refused() {
		let (taker, maker) = (UserId::new(), UserId::new());
		let asks = [resting(maker, "1.00", "3")];
		// Below the best ask: adds liquidity, rests whole.
		let rests = PriceTimeEngine.match_order(&incoming(taker, Side::Buy, Tif::Alo, "0.99", "10"), &asks, &open_policy(0)).unwrap();
		assert!(rests.fills.is_empty());
		assert_eq!(rests.remainder, shares("10"));
		assert!(rests.rests);
		// At the best ask: would take, so the whole order is refused rather than half-posted.
		assert_eq!(
			PriceTimeEngine
				.match_order(&incoming(taker, Side::Buy, Tif::Alo, "1.00", "10"), &asks, &open_policy(0))
				.unwrap_err(),
			Rejection::WouldCross
		);
	}

	#[test]
	fn crossing_your_own_order_refuses_the_whole_order() {
		let (me, other) = (UserId::new(), UserId::new());
		// My own ask is best; skipping it would let me jump my own queue, so it is a refusal
		// even though there is another maker behind it.
		let asks = [resting(me, "1.00", "3"), resting(other, "1.00", "3")];
		assert_eq!(
			PriceTimeEngine.match_order(&incoming(me, Side::Buy, Tif::Gtc, "1.00", "1"), &asks, &open_policy(0)).unwrap_err(),
			Rejection::SelfTrade
		);
		// My own resting order that does NOT cross is simply not reached.
		let asks = [resting(other, "1.00", "3"), resting(me, "1.50", "3")];
		let outcome = PriceTimeEngine.match_order(&incoming(me, Side::Buy, Tif::Gtc, "1.00", "1"), &asks, &open_policy(0)).unwrap();
		assert_eq!(outcome.fills.len(), 1);
	}

	#[test]
	fn a_market_order_is_an_ioc_limit_at_the_slipped_price() {
		// The use case prices a market buy off the best ask (1.00 → 1.05) and hands the
		// engine an IOC at that limit: it takes 1.00 and 1.04, stops before 1.06, and the
		// unfilled rest is released rather than rested.
		let (taker, maker) = (UserId::new(), UserId::new());
		let policy = open_policy(0);
		let limit = policy.market_limit(Side::Buy, price("1.00")).unwrap();
		let asks = [resting(maker, "1.00", "1"), resting(maker, "1.04", "1"), resting(maker, "1.06", "1")];
		let outcome = PriceTimeEngine
			.match_order(&incoming(taker, Side::Buy, Tif::Ioc, &limit.to_decimal_string(), "3"), &asks, &policy)
			.unwrap();
		assert_eq!(outcome.fills.len(), 2);
		assert_eq!(outcome.remainder, shares("1"));
		assert!(!outcome.rests);
	}

	#[test]
	fn an_order_fills_in_steps_and_releases_what_its_escrow_did_not_spend() {
		let policy = open_policy(100);
		let size = policy.size(shares("10")).unwrap();
		let reserve = policy.buy_reserve(shares("10"), price("2")).unwrap();
		assert_eq!(reserve, usdt("20.2"));
		let mut buy = Order::place(
			OrderId::new(),
			svc(),
			UserId::new(),
			ClientOrderId::parse("c1").unwrap(),
			Side::Buy,
			OrderKind::Limit,
			Tif::Gtc,
			price("2"),
			size,
			Locked::Cash(reserve),
		);
		assert_eq!(buy.state(), OrderState::Open);
		assert_eq!(buy.release(), None, "nothing is released while the order rests");

		// Fills at 1.90 (price improvement) as a taker paying 1 %: 4 units → 7.6 + 0.076.
		buy.fill(shares("4"), price("1.9"), usdt("0.076")).unwrap();
		assert_eq!(buy.state(), OrderState::PartiallyFilled);
		assert_eq!(buy.remaining(), shares("6"));
		assert_eq!(buy.release(), None);
		// The rest as a maker at its own limit, no fee: 6 × 2 = 12.
		buy.fill(shares("6"), price("2"), Usdt::ZERO).unwrap();
		assert_eq!(buy.state(), OrderState::Filled);
		assert_eq!(buy.notional_filled(), usdt("19.6"));
		assert_eq!(buy.fee_paid(), usdt("0.076"));
		assert_eq!(buy.average_fill_price(), Some(price("1.96")));
		// 20.2 reserved − 19.6 spent − 0.076 fee = 0.524 handed back.
		assert_eq!(buy.release(), Some(Locked::Cash(usdt("0.524"))));
		assert!(buy.fill(shares("1"), price("2"), Usdt::ZERO).is_err(), "a filled order takes no more");
		assert!(matches!(buy.cancel(), Err(DomainError::Conflict(_))));

		// A sell escrows its units and releases the unsold ones on cancel.
		let mut sell = Order::place(
			OrderId::new(),
			svc(),
			UserId::new(),
			ClientOrderId::parse("c2").unwrap(),
			Side::Sell,
			OrderKind::Limit,
			Tif::Gtc,
			price("2"),
			size,
			Locked::Units(shares("10")),
		);
		sell.fill(shares("3"), price("2"), Usdt::ZERO).unwrap();
		sell.cancel().unwrap();
		assert_eq!(sell.state(), OrderState::Cancelled);
		assert_eq!(sell.release(), Some(Locked::Units(shares("7"))));
		sell.cancel().unwrap();
		assert!(sell.fill(shares("1"), price("2"), Usdt::ZERO).is_err(), "a cancelled order takes no more");
		// A fully filled sell has nothing left to hand back.
		let mut sold = Order::place(
			OrderId::new(),
			svc(),
			UserId::new(),
			ClientOrderId::parse("c3").unwrap(),
			Side::Sell,
			OrderKind::Limit,
			Tif::Gtc,
			price("2"),
			size,
			Locked::Units(shares("10")),
		);
		sold.fill(shares("10"), price("2.5"), Usdt::ZERO).unwrap();
		assert_eq!(sold.release(), None);
	}

	#[test]
	fn a_fill_larger_than_the_remainder_is_refused() {
		let policy = open_policy(0);
		let mut order = Order::place(
			OrderId::new(),
			svc(),
			UserId::new(),
			ClientOrderId::parse("c").unwrap(),
			Side::Sell,
			OrderKind::Limit,
			Tif::Gtc,
			price("1"),
			policy.size(shares("1")).unwrap(),
			Locked::Units(shares("1")),
		);
		assert!(order.fill(shares("2"), price("1"), Usdt::ZERO).is_err());
		assert!(order.fill(Shares::ZERO, price("1"), Usdt::ZERO).is_err());
		assert_eq!(order.state(), OrderState::Open);
	}

	#[test]
	fn events_round_trip_through_json() {
		let events = [
			BookEvent::OrderPlaced {
				order_id: OrderId::new(),
				service: svc(),
				user: UserId::new(),
				side: Side::Buy,
				locked: Locked::Cash(usdt("10")),
			},
			BookEvent::TradeExecuted {
				trade_id: TradeId::new(),
				service: svc(),
				buyer: UserId::new(),
				seller: UserId::new(),
				buy_order_id: OrderId::new(),
				sell_order_id: OrderId::new(),
				taker_side: Side::Sell,
				size: shares("2"),
				price: price("1.5"),
				notional: usdt("3"),
				fee: usdt("0.003"),
				nav: Nav::SEED,
			},
			BookEvent::OrderReleased {
				order_id: OrderId::new(),
				service: svc(),
				user: UserId::new(),
				side: Side::Sell,
				released: Locked::Units(shares("1")),
			},
		];
		for event in events {
			let json = serde_json::to_string(&event).unwrap();
			let back: BookEvent = serde_json::from_str(&json).unwrap();
			assert_eq!(serde_json::to_string(&back).unwrap(), json);
		}
		assert_eq!(serde_json::to_string(&Locked::Units(shares("1"))).unwrap(), r#"{"kind":"units","amount":"1000000000000000000"}"#);
	}
}
