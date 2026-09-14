// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// The preview is a courtesy, but a wrong courtesy is worse than none: an operator told
// "no vote needed" who then finds the owners have been emailed has been misled about who
// is about to read what they wrote — and a rate the plane refuses at 501 bps should be
// refused here first, with words that say why.
import assert from "node:assert/strict";
import test from "node:test";

import { HOUSE_TERMS } from "../../../../shared/lib/fee-terms.ts";
import {
  draftProblem as draftProblemAt,
  draftRequirement,
  draftTerms,
  effectiveFromLifted,
  effectiveFromSeconds,
  localDateTimeValue,
  normalizeReason,
  toRequest,
  type TermsDraft,
} from "./schedule.ts";

// A clock held still, on a whole minute: `datetime-local` has no seconds, so a moment typed
// back from it must land exactly where it was read.
const NOW = Math.floor(Date.UTC(2026, 9, 1, 9, 0) / 1000);
const DAY = 86_400;
const draftProblem = (current: Parameters<typeof draftProblemAt>[0], draft: TermsDraft) => draftProblemAt(current, draft, NOW);
const typed = (seconds: number) => localDateTimeValue(new Date(seconds * 1000));

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

test("basis points typed into a percent field are refused as too large, not as not a percentage", () => {
  // "5001" used to fail the three-digit pattern and be called "not a percentage" — true
  // in no useful sense. The ceiling sentence names the mistake and its bound.
  assert.deepEqual(draftProblem(null, draft({ management: "1000" })), { key: "admin.fees.err.overCeiling", field: "management", ceiling: "5%" });
  assert.deepEqual(draftProblem(null, draft({ performance: "5001" })), { key: "admin.fees.err.overCeiling", field: "performance", ceiling: "50%" });
  assert.deepEqual(draftProblem(null, draft({ hurdle: "10001" })), { key: "admin.fees.err.overCeiling", field: "hurdle", ceiling: "100%" });
});

test("a rate typed with a decimal comma is the rate it says", () => {
  assert.deepEqual(draftTerms(draft({ management: "2,5", performance: "20,5", hurdle: "0,5" })), { ...HOUSE_TERMS, management_bps: 250, performance_bps: 2_050, hurdle_bps: 50 });
  assert.equal(toRequest("alpha", draft({ management: "2,5" })).management_bps, 250);
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
  const value = "2026-10-01T09:30";
  assert.equal(effectiveFromSeconds(value), Math.floor(new Date(value).getTime() / 1000));
  // The two directions agree to the minute, whatever zone the test runs in.
  assert.equal(effectiveFromSeconds(typed(NOW)), NOW);
  // Never a silent zero for something that was typed: "now" is not what they meant.
  assert.equal(effectiveFromSeconds("not a date"), null);
});

test("a moment more than 366 days ahead is refused here, with the horizon named", () => {
  assert.equal(draftProblem(HOUSE_TERMS, draft({ effectiveFrom: typed(NOW + 366 * DAY) })), null);
  assert.deepEqual(draftProblem(HOUSE_TERMS, draft({ effectiveFrom: typed(NOW + 367 * DAY) })), { key: "admin.fees.err.tooFarAhead", days: 366 });
  // The rate problems still come first.
  assert.deepEqual(draftProblem(HOUSE_TERMS, draft({ management: "6", effectiveFrom: typed(NOW + 367 * DAY) })), { key: "admin.fees.err.overCeiling", field: "management", ceiling: "5%" });
});

test("an early moment is not an error — the plane lifts it — but the field says so", () => {
  // Yesterday, and a moment inside the 24h notice, both go out as typed and come back lifted.
  assert.equal(draftProblem(HOUSE_TERMS, draft({ effectiveFrom: typed(NOW - DAY) })), null);
  assert.equal(effectiveFromLifted(typed(NOW - DAY), NOW), true);
  assert.equal(effectiveFromLifted(typed(NOW + DAY - 60), NOW), true);
  assert.equal(effectiveFromLifted(typed(NOW + DAY), NOW), false);
  assert.equal(effectiveFromLifted(typed(NOW + 30 * DAY), NOW), false);
  // Empty asks for the floor by name; it is not "lifted" from anything.
  assert.equal(effectiveFromLifted("", NOW), false);
  assert.equal(effectiveFromLifted("not a date", NOW), false);
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
