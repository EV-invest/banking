// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// The three strings below are copied from the money plane verbatim
// (`piggybank/core/src/application/consilium.rs` and `domain/src/consilium.rs`). If a
// backend rewording breaks one of these tests, that is the test doing its job: the screen
// falls back to the raw prose, which is safe but loses the translated explanation.
import assert from "node:assert/strict";
import test from "node:test";

import { classifyConsiliumRefusal, coolingOffLiftsAt } from "./consilium-refusal.ts";

// Stands in for `RequestError` rather than importing it: `shared/lib/api-client.ts` declares
// it with TypeScript parameter properties, which Node's strip-only type stripping (how this
// suite runs) cannot parse. The classifier only ever reads `.message`, and carrying a
// `status` here is the point of the first case — both refusals arrive with the same one.
class WireError extends Error {
  status = 409;
}

const MAIL =
  "governance mail is not configured, so no owner could be sent an approval token; a consilium opened now could never be voted on. Build with the `concierge_governance_mail` feature and configure the concierge mail relay first.";
const COOLING =
  "the owner roster changed less than 48h ago; a payout consilium cannot be opened until the cooling-off period lifts in 12h 30m";
const TOO_FEW = "a payout consilium needs at least 3 owners; this fund has 2, so the threshold can never be reached";

test("the two ALREADY_EXISTS refusals are told apart by message, not by status", () => {
  // They arrive with the same status, so the status cannot be the discriminator.
  assert.deepEqual(classifyConsiliumRefusal(new WireError(MAIL)), { kind: "mail-not-configured" });
  assert.deepEqual(classifyConsiliumRefusal(new WireError(COOLING)), { kind: "cooling-off", hours: 12, minutes: 30 });
});

test("the cooling-off deadline is extracted, not echoed", () => {
  const refusal = classifyConsiliumRefusal(COOLING);
  assert.equal(refusal?.kind, "cooling-off");
  assert.deepEqual(refusal, { kind: "cooling-off", hours: 12, minutes: 30 });
});

test("a cooling-off message whose shape changed still classifies, without a NaN", () => {
  // Losing the time is survivable — the screen names the condition without it. Rendering
  // "lifts in NaNh" is not.
  const refusal = classifyConsiliumRefusal("a payout consilium cannot be opened until the cooling-off period ends");
  assert.deepEqual(refusal, { kind: "cooling-off", hours: 0, minutes: 0 });
});

test("the deadline is an absolute moment taken once, from the duration", () => {
  // A duration re-based on each render drifts away from the deadline it describes.
  const now = Date.parse("2026-09-02T08:00:00.000Z");
  assert.equal(coolingOffLiftsAt({ hours: 12, minutes: 30 }, now), "2026-09-02T20:30:00.000Z");
  assert.equal(coolingOffLiftsAt({ hours: 0, minutes: 0 }, now), "2026-09-02T08:00:00.000Z");
});

test("a fund below the floor is recognised, with its owner count", () => {
  assert.deepEqual(classifyConsiliumRefusal(TOO_FEW), { kind: "too-few-owners", ownerCount: 2 });
});

test("anything else is left to the caller's existing error handling", () => {
  // Failing safe matters more than matching widely: an unrecognised message falls through
  // to the backend's own prose, which is what the screen showed before this existed.
  for (const other of [
    new WireError("Can't reach the server. Check your connection and try again."),
    new WireError("insufficient revenue"),
    new Error(""),
    "",
    null,
    undefined,
    42,
  ]) {
    assert.equal(classifyConsiliumRefusal(other), null, String(other));
  }
});
