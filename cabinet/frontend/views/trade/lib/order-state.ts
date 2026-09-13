// What an order's state means on screen. The wire has five states; the screen needs a
// sixth, because an IOC or market order that filled in part ends as `cancelled` with
// `filled > 0` — and "cancelled" alone would tell a trader who just bought 40 of 100
// units that nothing happened.
//
// Returns catalogue keys: this module is pure and has no translator. An unknown state
// falls back to the wire word, which is legible even when it is new.

import type { Order } from "@/shared/contracts/book";

import { toBaseUnits } from "../../../shared/lib/money.ts";

const STATE_KEYS: Record<string, string> = {
  open: "trade.state.open",
  partially_filled: "trade.state.partiallyFilled",
  filled: "trade.state.filled",
  cancelled: "trade.state.cancelled",
  rejected: "trade.state.rejected",
};

/** The catalogue key for the order's state, or `null` for a state this build has no word for. */
export function orderStateKey(order: Pick<Order, "state" | "filled">): string | null {
  if (order.state === "cancelled" && toBaseUnits(order.filled) > 0n) return "trade.state.partialCancelled";
  return STATE_KEYS[order.state ?? ""] ?? null;
}

/** Whether the order can still be cancelled — it is resting, in whole or in part. */
export function isResting(order: Pick<Order, "state">): boolean {
  return order.state === "open" || order.state === "partially_filled";
}
