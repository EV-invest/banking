// Run with `npm run test` (Node's built-in runner, native type-stripping).
import assert from "node:assert/strict";
import test from "node:test";

import { isResting, orderStateKey } from "./order-state.ts";

test("a cancelled order that filled in part is reported as partially filled, not as cancelled", () => {
  assert.equal(orderStateKey({ state: "cancelled", filled: "0" }), "trade.state.cancelled");
  assert.equal(orderStateKey({ state: "cancelled", filled: "40" }), "trade.state.partialCancelled");
  assert.equal(orderStateKey({ state: "cancelled", filled: "0.000000000000000001" }), "trade.state.partialCancelled");
  assert.equal(orderStateKey({ state: "filled", filled: "100" }), "trade.state.filled");
  assert.equal(orderStateKey({ state: "something_new", filled: "0" }), null);
});

test("only a resting order can be cancelled", () => {
  assert.equal(isResting({ state: "open" }), true);
  assert.equal(isResting({ state: "partially_filled" }), true);
  assert.equal(isResting({ state: "filled" }), false);
  assert.equal(isResting({ state: "cancelled" }), false);
});
