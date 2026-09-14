// Run with `npm run test` (Node's built-in runner, native type-stripping).
import assert from "node:assert/strict";
import test from "node:test";

import { receiptKind } from "./receipt.ts";

const NOW = 1_760_000_000;
const change = (over: Partial<{ state: string; effective_from: string; scheduled_at: string }> = {}) => ({
  state: "scheduled",
  effective_from: String(NOW + 86_400),
  scheduled_at: String(NOW),
  ...over,
});

test("a change the owners must carry awaits them, whatever moment it carries", () => {
  assert.equal(receiptKind(change({ state: "awaiting_consilium", effective_from: "0", scheduled_at: "0" }), NOW), "awaiting");
  assert.equal(receiptKind(change({ state: "awaiting_consilium" }), NOW), "awaiting");
});

test("a change with notice owed binds later and the holders are told", () => {
  assert.equal(receiptKind(change(), NOW), "scheduled");
  assert.equal(receiptKind(change({ effective_from: String(NOW + 30 * 86_400) }), NOW), "scheduled");
});

test("a product nobody holds binds at once — there is no one to notify", () => {
  // The plane sets the moment to the request itself when no notice is due.
  assert.equal(receiptKind(change({ effective_from: String(NOW), scheduled_at: String(NOW) }), NOW), "immediate");
  // Clocks drift: a moment already past reads the same way even if the two stamps differ.
  assert.equal(receiptKind(change({ effective_from: String(NOW - 5), scheduled_at: String(NOW - 6) }), NOW), "immediate");
});

test("a missing moment is never mistaken for 'now'", () => {
  assert.equal(receiptKind(change({ effective_from: "0", scheduled_at: "0" }), NOW), "scheduled");
  assert.equal(receiptKind(change({ effective_from: "", scheduled_at: "" }), NOW), "scheduled");
});
