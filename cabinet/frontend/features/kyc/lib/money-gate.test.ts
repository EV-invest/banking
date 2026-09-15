// Run with `npm run test` (Node's built-in runner, native type-stripping).
import assert from "node:assert/strict";
import test from "node:test";

import { isMoneyGated } from "./money-gate.ts";

test("a settled tier 0 closes the surface", () => {
  assert.equal(isMoneyGated({ level: 0, settled: true, loading: false }), true);
});

test("a settled tier 1 opens it", () => {
  assert.equal(isMoneyGated({ level: 1, settled: true, loading: false }), false);
});

test("nothing read yet is not a verdict", () => {
  // The surface shows a skeleton on this, so answering `true` here would flash the
  // verification block at a verified reader on every cold load.
  assert.equal(isMoneyGated({ level: 0, settled: false, loading: true }), false);
  assert.equal(isMoneyGated({ level: 0, settled: true, loading: true }), false);
});

test("a read that failed gates nothing", () => {
  // `level` defaults to 0 when there is no profile, so this is the case that would silently
  // take a verified reader's rails away over an unrelated outage.
  assert.equal(isMoneyGated({ level: 0, settled: false, loading: false }), false);
});
