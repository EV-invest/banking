// Run with `npm run test` (Node's built-in runner, native type-stripping).
import assert from "node:assert/strict";
import test from "node:test";

import { depthRows } from "./depth.ts";

test("totals accumulate from the best level down, exactly", () => {
  const { bids } = depthRows(
    [
      { price: "1.02", size: "0.1", orders: 1 },
      { price: "1.01", size: "0.2", orders: 2 },
      { price: "1.00", size: "0.000000000000000001", orders: 1 },
    ],
    [],
  );
  assert.deepEqual(
    bids.map((r) => r.total),
    // 0.1 + 0.2 is 0.3 here, not 0.30000000000000004.
    ["0.1", "0.3", "0.300000000000000001"],
  );
  assert.equal(bids[2]?.depth, 1);
});

test("both sides are scaled against the one deepest total", () => {
  const { bids, asks } = depthRows([{ price: "1.00", size: "300" }], [{ price: "1.01", size: "100" }, { price: "1.02", size: "50" }]);
  assert.equal(bids[0]?.depth, 1);
  assert.deepEqual(
    asks.map((r) => r.depth),
    [100 / 300, 150 / 300].map((d) => Math.floor(d * 10_000) / 10_000),
  );
});

test("an empty book draws no bars and does not divide by zero", () => {
  const { bids, asks } = depthRows(undefined, []);
  assert.deepEqual(bids, []);
  assert.deepEqual(asks, []);
  assert.equal(depthRows([{ price: "1", size: "0" }], []).bids[0]?.depth, 0);
});
