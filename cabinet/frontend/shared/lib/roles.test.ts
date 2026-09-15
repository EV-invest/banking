// Run with `npm run test` (Node's built-in runner, native type-stripping).
import assert from "node:assert/strict";
import test from "node:test";

import { visibleFor } from "./roles.ts";

// The wire strings, not an enum: the sidebar compares against what the session returns.
const FEES = ["admin", "owner"] as const;

test("an ungated surface shows to every role, and before the role is known", () => {
  assert.equal(visibleFor(undefined, "investor"), true);
  assert.equal(visibleFor(undefined, "operator"), true);
  assert.equal(visibleFor(undefined, undefined), true);
});

test("a gated surface shows only to the roles it names", () => {
  assert.equal(visibleFor(FEES, "admin"), true);
  assert.equal(visibleFor(FEES, "owner"), true);
  // banking#269: the operator saw the Fees entry and got a screen of 403s behind it.
  assert.equal(visibleFor(FEES, "operator"), false);
  assert.equal(visibleFor(FEES, "investor"), false);
});

test("a gated surface stays hidden while the session is still loading", () => {
  assert.equal(visibleFor(FEES, undefined), false);
});

test("an empty gate hides from everyone rather than being read as ungated", () => {
  assert.equal(visibleFor([], "owner"), false);
});
