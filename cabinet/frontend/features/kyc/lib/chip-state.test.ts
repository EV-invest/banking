// Run with `npm run test` (Node's built-in runner, native type-stripping).
import assert from "node:assert/strict";
import test from "node:test";

import { kycChipState } from "./chip-state.ts";

test("tier 0 with nothing running has not started", () => {
  assert.deepEqual(kycChipState({ level: 0, runningCase: null }), { state: "notStarted", tone: "muted", labelKey: "kyc.chip.notStarted" });
});

test("a running case is in review, whatever word the plane uses for it", () => {
  for (const status of ["pending", "in_progress", "in_review", "a_status_shipped_after_this_cabinet"]) {
    assert.equal(kycChipState({ level: 0, runningCase: { status } }).state, "review", status);
  }
  assert.equal(kycChipState({ level: 0, runningCase: { status: "in_review" } }).tone, "neutral");
});

test("a case sent back to the reader needs their attention", () => {
  assert.deepEqual(kycChipState({ level: 0, runningCase: { status: "resubmitted" } }), { state: "attention", tone: "warn", labelKey: "kyc.chip.attention" });
});

test("the tier outranks the case", () => {
  // A case row can lag the tier the hub already acts on; the chip must not tell a verified
  // reader they are still waiting.
  assert.equal(kycChipState({ level: 1, runningCase: { status: "in_review" } }).state, "verified");
  assert.deepEqual(kycChipState({ level: 2, runningCase: null }), { state: "verified", tone: "positive", labelKey: "kyc.chip.verified" });
});
