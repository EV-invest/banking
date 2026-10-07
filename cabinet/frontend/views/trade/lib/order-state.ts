// What an order's state means on screen. The wire has five states; the screen needs
// three words for one of them. A `cancelled` order is either the trader's own cancel or
// the hub's — an IOC or market order ending with what could not cross — and the hub's
// splits again on whether anything crossed first: "cancelled" alone would tell a trader
// who just bought 40 of 100 units that nothing happened. The wire's `cancel_reason`
// decides which, not `filled` — a trader who pulls a half-filled GTC order gets
// "Cancelled", because that is what they did.
//
// Returns catalogue keys: this module is pure and has no translator. An unknown state
// falls back to the wire word, which is legible even when it is new.

import type { Translate } from "@evinvest/i18n";

import type { Order } from "@/shared/contracts/book";

import { isOrderCancelReason } from "../../../entities/book/lib/vocabulary.ts";
import { toBaseUnits } from "../../../shared/lib/money.ts";

export type OrderStateKey =
  | "trade.state.open"
  | "trade.state.partiallyFilled"
  | "trade.state.filled"
  | "trade.state.cancelled"
  | "trade.state.rejected"
  | "trade.state.partialCancelled"
  | "trade.state.unfilledCancelled";

const STATE_KEYS: Readonly<Record<string, OrderStateKey>> = {
  open: "trade.state.open",
  partially_filled: "trade.state.partiallyFilled",
  filled: "trade.state.filled",
  cancelled: "trade.state.cancelled",
  rejected: "trade.state.rejected",
};

type StateFields = Pick<Order, "state" | "filled" | "cancel_reason">;

/** The catalogue key for the order's state, or `null` for a state this build has no word for. */
export function orderStateKey(order: StateFields): OrderStateKey | null {
  if (order.state === "cancelled" && isOrderCancelReason(order.cancel_reason) && order.cancel_reason !== "user") {
    return toBaseUnits(order.filled) > 0n ? "trade.state.partialCancelled" : "trade.state.unfilledCancelled";
  }
  const state = order.state ?? "";
  return Object.hasOwn(STATE_KEYS, state) ? STATE_KEYS[state] : null;
}

const stateWords = (t: Translate): Record<OrderStateKey, string> => ({
  "trade.state.open": t("trade.state.open", "Open"),
  "trade.state.partiallyFilled": t("trade.state.partiallyFilled", "Partially filled"),
  "trade.state.filled": t("trade.state.filled", "Filled"),
  "trade.state.cancelled": t("trade.state.cancelled", "Cancelled"),
  "trade.state.rejected": t("trade.state.rejected", "Rejected"),
  "trade.state.partialCancelled": t("trade.state.partialCancelled", "Partially filled, rest cancelled"),
  "trade.state.unfilledCancelled": t("trade.state.unfilledCancelled", "Not filled, cancelled"),
});

/** The order's state in words, or `null` for a state this build has no word for. */
export function orderStateLabel(order: StateFields, t: Translate): string | null {
  const key = orderStateKey(order);
  return key ? stateWords(t)[key] : null;
}

/** Which `trade.form.placed.*` sentence describes a just-placed order's answer. */
export type PlacedOutcome = "rejected" | "partial" | "resting" | "filled" | "cancelled";

export function placedOutcome(order: StateFields): PlacedOutcome {
  if (order.state === "rejected") return "rejected";
  if (isResting(order)) return "resting";
  if (order.state === "filled") return "filled";
  return orderStateKey(order) === "trade.state.partialCancelled" ? "partial" : "cancelled";
}

/** Whether the order can still be cancelled — it is resting, in whole or in part. */
export function isResting(order: Pick<Order, "state">): boolean {
  return order.state === "open" || order.state === "partially_filled";
}
