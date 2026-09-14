// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// The preview is a courtesy, but a wrong courtesy is worse than none: an operator told
// "no vote needed" who then finds the owners have been emailed has been misled about who
// is about to read what they wrote — and a rate the plane refuses at 501 bps should be
// refused here first, with words that say why.
import assert from "node:assert/strict";
import test from "node:test";

import { HOUSE_TERMS } from "../../../../shared/lib/fee-terms.ts";
import { draftProblem, draftRequirement, draftTerms, effectiveFromSeconds, normalizeReason, toRequest, type TermsDraft } from "./schedule.ts";

const draft = (over: Partial<TermsDraft> = {}): TermsDraft => ({
  management: "2",
  performance: "20",
  hurdle: "0",
  basis: "invested_capital",
  crystallization: "annual",
  effectiveFrom: "",
  reason: "",
  ...over,
});

test("the house draft parses to the house terms", () => {
  assert.deepEqual(draftTerms(draft()), HOUSE_TERMS);
});

test("a rate that is not a percentage is the first problem named, in field order", () => {
  assert.deepEqual(draftProblem(null, draft({ management: "" })), { key: "admin.fees.err.notPercent", field: "management" });
  assert.deepEqual(draftProblem(null, draft({ performance: "abc", hurdle: "x" })), { key: "admin.fees.err.notPercent", field: "performance" });
  assert.equal(draftTerms(draft({ hurdle: "2%" })), null);
});

test("the ceilings are 5% p.a., 50% of the gain and 100% for the hurdle — inclusive", () => {
  // A rate at the ceiling sits outside the envelope, so these carry the reason the owners
  // will then require; what is under test is the ceiling alone.
  const reason = "Costs rose.";
  assert.equal(draftProblem(null, draft({ management: "5", reason })), null);
  assert.deepEqual(draftProblem(null, draft({ management: "5.01", reason })), { key: "admin.fees.err.overCeiling", field: "management", ceiling: "5%" });
  assert.equal(draftProblem(null, draft({ performance: "50", reason })), null);
  assert.deepEqual(draftProblem(null, draft({ performance: "50.5", reason })), { key: "admin.fees.err.overCeiling", field: "performance", ceiling: "50%" });
  assert.equal(draftProblem(null, draft({ hurdle: "100" })), null);
  assert.deepEqual(draftProblem(null, draft({ hurdle: "100.01" })), { key: "admin.fees.err.overCeiling", field: "hurdle", ceiling: "100%" });
  // The old field admitted anything up to 100% for every rate; 20% p.a. is now refused.
  assert.equal(draftTerms(draft({ management: "20" })), null);
});

test("the requirement is unknown until every rate parses, then follows the plane's rule", () => {
  assert.equal(draftRequirement(HOUSE_TERMS, draft({ management: "" })), null);
  assert.equal(draftRequirement(HOUSE_TERMS, draft()), "admin");
  assert.equal(draftRequirement(HOUSE_TERMS, draft({ management: "3" })), "owner_consilium");
  assert.equal(draftRequirement(HOUSE_TERMS, draft({ crystallization: "quarterly" })), "owner_consilium");
  assert.equal(draftRequirement({ ...HOUSE_TERMS, hurdle_bps: 500 }, draft({ hurdle: "4" })), "owner_consilium");
  assert.equal(draftRequirement(null, draft()), "admin");
});

test("a reason is required exactly when the owners must approve", () => {
  assert.deepEqual(draftProblem(HOUSE_TERMS, draft({ management: "3" })), { key: "admin.fees.err.reasonRequired" });
  assert.deepEqual(draftProblem(HOUSE_TERMS, draft({ management: "3", reason: "   " })), { key: "admin.fees.err.reasonRequired" });
  assert.equal(draftProblem(HOUSE_TERMS, draft({ management: "3", reason: "Costs rose." })), null);
  // An administrator's change may carry no reason at all.
  assert.equal(draftProblem(HOUSE_TERMS, draft({ management: "1" })), null);
  // The rate problems come first: a reason cannot excuse a rate the plane will refuse.
  assert.deepEqual(draftProblem(HOUSE_TERMS, draft({ management: "6" })), { key: "admin.fees.err.overCeiling", field: "management", ceiling: "5%" });
});

test("the reason is measured in bytes, as the plane measures it", () => {
  assert.equal(draftProblem(HOUSE_TERMS, draft({ reason: "a".repeat(500) })), null);
  assert.deepEqual(draftProblem(HOUSE_TERMS, draft({ reason: "a".repeat(501) })), { key: "admin.fees.err.reasonTooLong", max: 500, used: 501 });
  // 251 Cyrillic letters are 502 bytes: the letter count alone would have passed this.
  assert.deepEqual(draftProblem(HOUSE_TERMS, draft({ reason: "ж".repeat(251) })), { key: "admin.fees.err.reasonTooLong", max: 500, used: 502 });
  // Measured after normalisation: the surrounding whitespace is never sent.
  assert.equal(draftProblem(HOUSE_TERMS, draft({ reason: `  ${"a".repeat(500)}  ` })), null);
  // A too-long reason is refused whoever has to agree, and before "required" is asked.
  assert.deepEqual(draftProblem(HOUSE_TERMS, draft({ management: "3", reason: "a".repeat(501) })), { key: "admin.fees.err.reasonTooLong", max: 500, used: 501 });
});

test("a pasted line break is folded into a space rather than refused by the plane", () => {
  assert.equal(normalizeReason("Costs\nrose."), "Costs rose.");
  assert.equal(normalizeReason("Costs\r\n\trose.  "), "Costs rose.");
  assert.equal(toRequest("alpha", draft({ reason: "Costs\nrose." })).reason, "Costs rose.");
  // A reason that is nothing but breaks is no reason at all.
  assert.deepEqual(draftProblem(HOUSE_TERMS, draft({ management: "3", reason: "\n\n" })), { key: "admin.fees.err.reasonRequired" });
});

test("an empty moment asks for the earliest allowed; a typed one is read in the local zone", () => {
  assert.equal(effectiveFromSeconds(""), 0);
  assert.equal(effectiveFromSeconds("   "), 0);
  const typed = "2026-10-01T09:30";
  assert.equal(effectiveFromSeconds(typed), Math.floor(new Date(typed).getTime() / 1000));
  // Never a silent zero for something that was typed: "now" is not what they meant.
  assert.equal(effectiveFromSeconds("not a date"), null);
});

test("the request carries the parsed terms, the moment and the trimmed reason", () => {
  assert.deepEqual(toRequest("alpha", draft({ management: "2.5", reason: "  Because.  " })), {
    service: "alpha",
    management_bps: 250,
    performance_bps: 2_000,
    hurdle_bps: 0,
    basis: "invested_capital",
    crystallization: "annual",
    effective_from: 0,
    reason: "Because.",
  });
});
