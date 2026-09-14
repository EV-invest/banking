// The secondary market in an allocation's units — the hub's central limit order book,
// bridged by the BFF under `/api/book/*` (see `cabinet/backend/README.md` § The book
// surface). The message shapes are the proto's, re-exported from `./gen` under the names
// the cabinet uses; the socket frame is the BFF's own envelope around them and is written
// by hand here, the way `./governance` does for its frame.
//
// Every amount, price, size, timestamp and revision crosses as a STRING (the BFF renders
// 64-bit values that way). `number | string` on a few fields is the generator's spelling of
// an int64 — read them through `String()` and never do arithmetic on them raw.

export type {
  BankingV1BookSnapshot as BookSnapshot,
  BankingV1BookLevel as BookLevel,
  BankingV1BookPolicy as BookPolicy,
  BankingV1Order as Order,
  BankingV1OrderList as OrderList,
  BankingV1Trade as Trade,
  BankingV1TradeList as TradeList,
  BankingV1Candle as Candle,
  BankingV1CandleList as CandleList,
  BankingV1BookEvent as BookEvent,
} from "./gen";

/** `buy` takes units for cash, `sell` the reverse. The wire's own words. */
export type OrderSide = "buy" | "sell";

/** A `limit` rests at its price; a `market` crosses whatever is there, capped by the
 *  policy's slippage. */
export type OrderKind = "limit" | "market";

/** `gtc` rests until filled or cancelled, `ioc` fills what it can and cancels the rest,
 *  `alo` (post-only) is refused rather than crossing the spread. */
export type OrderTif = "gtc" | "ioc" | "alo";

/** Why a `cancelled` order is cancelled — `""` on every other state. `user` is the owner's
 *  own cancel; the other two are the hub cancelling what an IOC limit / a market order
 *  could not fill at once, which is how a cancelled order can still carry `filled > 0`. */
export type OrderCancelReason = "user" | "ioc_remainder" | "market_remainder";

/** The candle buckets the hub aggregates; the wire's own vocabulary. */
export type CandleResolution = "1m" | "5m" | "15m" | "1h" | "4h" | "1d";

/** The place-order body, exactly as `POST /api/book/orders` reads it. `price` is absent
 *  for a market order (the hub derives the limit from the quote); `tif` is absent for one
 *  too, since a market order is by nature immediate-or-cancel. */
export interface PlaceOrderBody {
  service: string;
  side: OrderSide;
  kind: OrderKind;
  tif?: OrderTif;
  price?: string;
  size: string;
  /** 1..64 chars, unique per service — the retry key. A resend of the same intent carries
   *  the same id and lands one order (a reuse answers 409). */
  client_order_id: string;
}

/** The admin body for `POST /api/admin/allocations/book`. The three optional fields are
 *  sent only when the operator set them: absent means "keep the hub's value". */
export interface SetBookPolicyBody {
  service: string;
  book_open: boolean;
  taker_fee_bps: number;
  price_tick?: string;
  lot_size?: string;
  market_slippage_bps?: number;
}

// ── The realtime frame ─────────────────────────────────────────────────────────

import type { BankingV1BookSnapshot, BankingV1Trade } from "./gen";

/**
 * Everything `/api/book/ws` is allowed to say.
 *
 * Unlike the governance socket this one IS a delivery: the frame carries the same snapshot
 * `GET /api/book` answers and the latest public trades, so the book on screen is rendered
 * from it directly. Nothing personal rides on it — no balance and no other user's orders.
 * The caller's own orders are still a REST read: `orders_revision` only says WHEN to
 * re-issue it.
 */
export interface BookFrame {
  type: "book";
  snapshot?: BankingV1BookSnapshot;
  /** Newest first. */
  trades?: BankingV1Trade[];
  /** The book revision at which the caller's own orders last changed; "0" = never. */
  orders_revision?: number | string;
}

export interface HeartbeatFrame {
  type: "heartbeat";
  at?: string;
}

export type BookSocketFrame = BookFrame | HeartbeatFrame;
