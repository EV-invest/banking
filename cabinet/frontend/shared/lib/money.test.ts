// Run with `npm run test` (Node's built-in runner, native type-stripping).
import assert from "node:assert/strict";
import test from "node:test";

import { compactUnits, fractionOfCap } from "./money.ts";

// The sizes this feature actually runs at: a hundred-million-unit cap is 1e26 base units,
// past the 2^53 integer precision of a double. `Number(issued) / Number(cap)` is the
// obvious implementation and the wrong one.
const CAP = "100000000";

test("a supply fraction stays exact at cap sizes a float cannot hold", () => {
  assert.equal(fractionOfCap("50000000", CAP), 0.5);
  assert.equal(fractionOfCap("25000000", CAP), 0.25);
  // 90% is the threshold the bar turns amber on, so it has to land exactly.
  assert.equal(fractionOfCap("90000000", CAP), 0.9);
});

test("a small holding is a small number, not a flat zero", () => {
  // The regression: dividing at basis-point scale floored one unit of a 100M cap to 0,
  // which is indistinguishable from "nothing issued".
  assert.ok(fractionOfCap("1", CAP) > 0, "one unit of 100M must not round away");
  assert.equal(fractionOfCap("1", CAP), 1e-8);
  assert.equal(fractionOfCap("5000", CAP), 5e-5);
  // Genuinely nothing issued is still exactly zero.
  assert.equal(fractionOfCap("0", CAP), 0);
});

test("a fraction is clamped to 1 and never divides by a zero cap", () => {
  // A cap narrowed below the issued supply is legal; the bar must read full, not >100%.
  assert.equal(fractionOfCap("500", "100"), 1);
  assert.equal(fractionOfCap("100", "100"), 1);
  // An absent or zero cap has no meaningful fraction — 0, not NaN or Infinity.
  assert.equal(fractionOfCap("100", "0"), 0);
  assert.equal(fractionOfCap("100", undefined), 0);
  assert.equal(fractionOfCap(undefined, CAP), 0);
});

test("unit counts compact only once they stop being readable", () => {
  // A fund sized to hundreds of units must still read as its own number.
  assert.equal(compactUnits("500"), "500.00");
  assert.equal(compactUnits("999"), "999.00");
  assert.equal(compactUnits("1000"), "1K");
  assert.equal(compactUnits("940000"), "940K");
  assert.equal(compactUnits("100000000"), "100M");
  assert.equal(compactUnits("21000000"), "21M");
});
