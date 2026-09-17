// Run with `npm run test` (Node's built-in runner, native type-stripping).
import assert from "node:assert/strict";
import test from "node:test";

import { hasHoldings } from "./holdings.ts";

test("unread positions are unknown, not empty", () => {
  assert.equal(hasHoldings(undefined), null);
});

test("no positions, or positions of zero units, hold nothing", () => {
  assert.equal(hasHoldings({}), false);
  assert.equal(hasHoldings({ positions: [] }), false);
  assert.equal(hasHoldings({ positions: [{ service: "arb", units: "0" }, { service: "re" }] }), false);
});

test("any position above zero units is a holding", () => {
  assert.equal(hasHoldings({ positions: [{ service: "arb", units: "0" }, { service: "re", units: "0.000001" }] }), true);
});
