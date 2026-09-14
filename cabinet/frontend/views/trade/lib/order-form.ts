// The order form's model: what the trader has typed, whether it can be sent, what it
// costs, and the exact wire body it becomes. Pure and React-free, so the rules the hub
// enforces with a 400 — tick, lot, funds — are stated here before the submit and tested
// here rather than discovered against the BFF.
//
// Every figure is exact bigint arithmetic on the wire's decimal strings; the previews are
// display-only and the hub re-derives all of them, but a preview that disagrees with the
// ledger by a rounding error is still a preview that lied.

import type { BookPolicy, OrderKind, OrderSide, OrderTif, PlaceOrderBody } from "@/shared/contracts/book";

import { fromBaseUnits, toBaseUnits, USDT_DECIMALS } from "../../../shared/lib/money.ts";

export interface OrderDraft {
  side: OrderSide;
  kind: OrderKind;
  tif: OrderTif;
  /** Ignored for a market order — the hub derives the limit from the quote. */
  price: string;
  size: string;
}

export const EMPTY_ORDER_DRAFT: OrderDraft = { side: "buy", kind: "limit", tif: "gtc", price: "", size: "" };

/** What the form knows about the world the draft is sent into. */
export interface OrderContext {
  policy: Pick<BookPolicy, "taker_fee_bps" | "price_tick" | "lot_size"> | null;
  /** Cash available to a buy, decimal USDT — or `null` while the wallet has not arrived. */
  availableCash: string | null;
  /** Free units available to a sell — or `null` while the position has not arrived. */
  availableUnits: string | null;
  /** What a market order would trade around: the best opposite level, else the last
   *  print. `null` when the book is empty, which is exactly when a market order is refused. */
  referencePrice: string | null;
}

/** Why the draft cannot be sent, in the order the form should point at. `null` = send it. */
export type OrderDraftProblem = "price" | "tick" | "size" | "lot" | "funds" | "noQuote";

const DECIMAL = /^\d+(\.\d+)?$/;
const SCALE = 10n ** BigInt(USDT_DECIMALS);

const isPositive = (raw: string): boolean => DECIMAL.test(raw.trim()) && toBaseUnits(raw) > 0n;

/** Whether `value` is a whole multiple of `step`. A step of zero or none means "any". */
function onGrid(value: string, step: string | undefined): boolean {
  const s = toBaseUnits(step);
  return s <= 0n || toBaseUnits(value) % s === 0n;
}

/** The price the order is valued at for the preview: the limit, or the reference for a market order. */
export function effectivePrice(draft: OrderDraft, context: OrderContext): string | null {
  if (draft.kind === "market") return context.referencePrice;
  return isPositive(draft.price) ? draft.price.trim() : null;
}

/** `price × size` in base units, or `null` while either is missing. */
export function orderNotional(draft: OrderDraft, context: OrderContext): bigint | null {
  const price = effectivePrice(draft, context);
  if (price === null || !isPositive(draft.size)) return null;
  return (toBaseUnits(price) * toBaseUnits(draft.size)) / SCALE;
}

/**
 * The taker fee on the notional, `notional × bps / 10000`. A post-only order can only ever
 * make, so its fee is zero by construction; a resting limit that later gets lifted pays
 * nothing either, but the form cannot know whether a limit will cross, so it shows the
 * taker figure as the ceiling.
 */
export function takerFee(notional: bigint, draft: OrderDraft, policy: OrderContext["policy"]): bigint {
  if (draft.tif === "alo" && draft.kind === "limit") return 0n;
  return (notional * BigInt(policy?.taker_fee_bps ?? 0)) / 10_000n;
}

export function orderDraftProblem(draft: OrderDraft, context: OrderContext): OrderDraftProblem | null {
  if (draft.kind === "limit") {
    if (!isPositive(draft.price)) return "price";
    if (!onGrid(draft.price, context.policy?.price_tick)) return "tick";
  } else if (context.referencePrice === null) {
    return "noQuote";
  }
  if (!isPositive(draft.size)) return "size";
  if (!onGrid(draft.size, context.policy?.lot_size)) return "lot";
  if (draft.side === "sell") {
    if (context.availableUnits !== null && toBaseUnits(draft.size) > toBaseUnits(context.availableUnits)) return "funds";
  } else {
    const notional = orderNotional(draft, context);
    // A buy escrows the notional plus the fee it may pay as taker.
    if (notional !== null && context.availableCash !== null && notional + takerFee(notional, draft, context.policy) > toBaseUnits(context.availableCash)) return "funds";
  }
  return null;
}

/** The wire body for a sendable draft, or `null` — the button is disabled on the same rule. */
export function placeOrderBody(service: string, draft: OrderDraft, context: OrderContext, clientOrderId: string): PlaceOrderBody | null {
  if (orderDraftProblem(draft, context) !== null) return null;
  const base = { service, side: draft.side, kind: draft.kind, size: draft.size.trim(), client_order_id: clientOrderId };
  // A market order carries neither price nor tif: the hub derives the limit from the quote
  // and a market order is immediate by nature. Sending `tif: "gtc"` with it would be
  // asking to rest something that cannot rest.
  return draft.kind === "market" ? base : { ...base, tif: draft.tif, price: draft.price.trim() };
}

/**
 * The retry contract: one `client_order_id` per distinct intent, the SAME id on a retry
 * of it. A draft edited after a failure is a new decision and gets a new id; a resend of
 * the identical body after a timeout must reuse the old one, or the double click the id
 * exists to absorb lands two orders (the hub answers 409 to a reuse, which is the safe
 * side). Same shape as the issuance form's `SubmissionKey`.
 */
export interface OrderSubmission {
  id: string;
  fingerprint: string;
}

export function orderFingerprint(service: string, draft: OrderDraft): string {
  return JSON.stringify([service, draft.side, draft.kind, draft.kind === "market" ? "" : draft.tif, draft.kind === "market" ? "" : draft.price.trim(), draft.size.trim()]);
}

export function orderSubmissionFor(previous: OrderSubmission | null, service: string, draft: OrderDraft, mint: () => string = () => crypto.randomUUID()): OrderSubmission {
  const fingerprint = orderFingerprint(service, draft);
  if (previous && previous.fingerprint === fingerprint) return previous;
  return { id: mint(), fingerprint };
}

/** Base units back to the decimal string the formatters take. */
export const asDecimal = fromBaseUnits;
