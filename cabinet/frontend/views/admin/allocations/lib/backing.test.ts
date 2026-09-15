// Run with `npm run test` (Node's built-in runner, native type-stripping).
import assert from "node:assert/strict";
import test from "node:test";

import { backingOf, oppositeBacking } from "./backing.ts";

test("an absent backing is the hub's default, cash — not an unknown", () => {
  assert.equal(backingOf({}), "cash");
  assert.equal(backingOf({ backing: undefined }), "cash");
  assert.equal(backingOf({ backing: "cash" }), "cash");
  assert.equal(backingOf({ backing: "in_kind" }), "in_kind");
});

test("the flip is the other value, both ways", () => {
  assert.equal(oppositeBacking("cash"), "in_kind");
  assert.equal(oppositeBacking("in_kind"), "cash");
});
