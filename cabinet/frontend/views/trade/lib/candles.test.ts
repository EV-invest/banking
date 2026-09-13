// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// The live update is `series.update` on the last bar, never a `setData` of the whole
// history — so what a trade does to that bar has to be right on its own.
import assert from "node:assert/strict";
import test from "node:test";

import { applyTrade, barFromCandle, bucketStart, type Bar } from "./candles.ts";

const T0 = 1_700_000_000; // a 1m bucket boundary is 1_699_999_980; 5m is 1_699_999_800

test("a moment falls into the bucket that starts at or before it", () => {
  assert.equal(bucketStart(T0, "1m"), 1_699_999_980);
  assert.equal(bucketStart(T0, "5m"), 1_699_999_800);
  assert.equal(bucketStart(1_699_999_980, "1m"), 1_699_999_980);
});

test("a wire candle becomes a bar; an empty bucket becomes nothing", () => {
  assert.deepEqual(barFromCandle({ time: "1700000000", open: "1", high: "1.5", low: "0.5", close: "1.2", volume: "3" }), {
    time: 1_700_000_000,
    open: "1",
    high: "1.5",
    low: "0.5",
    close: "1.2",
    volume: "3",
  });
  assert.equal(barFromCandle({ time: "1700000000" }), null);
});

test("a trade in the open bucket moves the close, widens the range and adds volume", () => {
  const last: Bar = { time: 1_699_999_980, open: "1.00", high: "1.05", low: "0.99", close: "1.02", volume: "0.1" };
  const bar = applyTrade(last, { price: "1.07", size: "0.2", executed_at: String(T0) }, "1m");
  assert.deepEqual(bar, { ...last, high: "1.07", close: "1.07", volume: "0.3" });
  const lower = applyTrade(bar, { price: "0.98", size: "1", executed_at: String(T0 + 5) }, "1m");
  assert.equal(lower?.low, "0.98");
  assert.equal(lower?.high, "1.07");
  assert.equal(lower?.volume, "1.3");
});

test("a trade in a later bucket opens a new bar at its price", () => {
  const last: Bar = { time: 1_699_999_980, open: "1.00", high: "1.05", low: "0.99", close: "1.02", volume: "0.1" };
  const bar = applyTrade(last, { price: "1.03", size: "2", executed_at: String(T0 + 60) }, "1m");
  assert.deepEqual(bar, { time: 1_700_000_040, open: "1.03", high: "1.03", low: "1.03", close: "1.03", volume: "2" });
  // With no bar yet, the first trade opens one.
  assert.equal(applyTrade(null, { price: "1.03", size: "2", executed_at: String(T0) }, "1m")?.time, 1_699_999_980);
});

test("a trade from a bucket that has already closed is ignored, not rewritten", () => {
  const last: Bar = { time: 1_700_000_040, open: "1.03", high: "1.03", low: "1.03", close: "1.03", volume: "2" };
  assert.equal(applyTrade(last, { price: "9", size: "1", executed_at: String(T0) }, "1m"), null);
  assert.equal(applyTrade(last, { size: "1", executed_at: String(T0 + 60) }, "1m"), null);
});
