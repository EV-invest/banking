// Run with `npm run test` (Node's built-in runner, native type-stripping).
import assert from "node:assert/strict";
import test from "node:test";

import { isResting, orderStateKey, placedOutcome } from "./order-state.ts";

test("a cancelled order is worded by why it was cancelled, not by how much filled", () => {
  // The trader's own cancel is a cancel, however much had crossed by then.
  assert.equal(orderStateKey({ state: "cancelled", cancel_reason: "user", filled: "0" }), "trade.state.cancelled");
  assert.equal(orderStateKey({ state: "cancelled", cancel_reason: "user", filled: "40" }), "trade.state.cancelled");
  // The hub's remainder cancels split on whether anything crossed first.
  assert.equal(orderStateKey({ state: "cancelled", cancel_reason: "ioc_remainder", filled: "40" }), "trade.state.partialCancelled");
  assert.equal(orderStateKey({ state: "cancelled", cancel_reason: "market_remainder", filled: "0.000000000000000001" }), "trade.state.partialCancelled");
  assert.equal(orderStateKey({ state: "cancelled", cancel_reason: "ioc_remainder", filled: "0" }), "trade.state.unfilledCancelled");
  assert.equal(orderStateKey({ state: "cancelled", cancel_reason: "market_remainder", filled: "" }), "trade.state.unfilledCancelled");
  // A reason this build has no word for — or none at all — reads as a plain cancel.
  assert.equal(orderStateKey({ state: "cancelled", cancel_reason: "", filled: "40" }), "trade.state.cancelled");
  assert.equal(orderStateKey({ state: "cancelled", cancel_reason: "something_new", filled: "40" }), "trade.state.cancelled");
  assert.equal(orderStateKey({ state: "cancelled", filled: "40" }), "trade.state.cancelled");
});

test("the reason only matters on a cancelled order", () => {
  assert.equal(orderStateKey({ state: "filled", cancel_reason: "", filled: "100" }), "trade.state.filled");
  assert.equal(orderStateKey({ state: "open", cancel_reason: "", filled: "0" }), "trade.state.open");
  assert.equal(orderStateKey({ state: "partially_filled", cancel_reason: "", filled: "10" }), "trade.state.partiallyFilled");
  assert.equal(orderStateKey({ state: "rejected", cancel_reason: "", filled: "0" }), "trade.state.rejected");
  assert.equal(orderStateKey({ state: "something_new", cancel_reason: "", filled: "0" }), null);
});

test("a just-placed order's answer picks the sentence by the same reason", () => {
  assert.equal(placedOutcome({ state: "rejected", cancel_reason: "", filled: "0" }), "rejected");
  assert.equal(placedOutcome({ state: "open", cancel_reason: "", filled: "0" }), "resting");
  assert.equal(placedOutcome({ state: "partially_filled", cancel_reason: "", filled: "10" }), "resting");
  assert.equal(placedOutcome({ state: "filled", cancel_reason: "", filled: "100" }), "filled");
  assert.equal(placedOutcome({ state: "cancelled", cancel_reason: "ioc_remainder", filled: "40" }), "partial");
  assert.equal(placedOutcome({ state: "cancelled", cancel_reason: "ioc_remainder", filled: "0" }), "cancelled");
  assert.equal(placedOutcome({ state: "cancelled", cancel_reason: "market_remainder", filled: "0" }), "cancelled");
  assert.equal(placedOutcome({ state: "cancelled", cancel_reason: "user", filled: "40" }), "cancelled");
});

test("only a resting order can be cancelled", () => {
  assert.equal(isResting({ state: "open" }), true);
  assert.equal(isResting({ state: "partially_filled" }), true);
  assert.equal(isResting({ state: "filled" }), false);
  assert.equal(isResting({ state: "cancelled" }), false);
});
