// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// The preview is a courtesy, but a wrong courtesy is worse than none: an operator told
// "the owners will be asked" who then emails one investor has been misled about who is
// about to read the reason they wrote. These pin the two derivations to the plane's rules.
import assert from "node:assert/strict";
import test from "node:test";

import { EMPTY_END, PARTY_KINDS, draftProblem, previewRequirement, previewTier, toRequest, type EndDraft } from "./terms.ts";

const end = (over: Partial<EndDraft>): EndDraft => ({ ...EMPTY_END, ...over });
// The platform's own money is the reserved allocations, addressed as a product (#245).
const fee = end({ kind: "service", id: "fee" });
const fund = end({ kind: "service", id: "fund" });

test("the retired singletons are not on offer — the plane refuses them by name", () => {
  assert.deepEqual([...PARTY_KINDS], ["service", "user"]);
});

test("the tier follows the destination alone", () => {
  assert.equal(previewTier(end({ kind: "external", network: "bep20", address: "0xabc" })), "external");
  assert.equal(previewTier(end({ kind: "service", id: "alpha" })), "service");
  assert.equal(previewTier(fee), "service");
  assert.equal(previewTier(end({ kind: "user", id: "u1" })), "internal");
});

test("the requirement follows the source alone", () => {
  assert.equal(previewRequirement(end({ kind: "user", id: "u1" })), "subject_consent");
  assert.equal(previewRequirement(end({ kind: "service", id: "alpha" })), "owner_consilium");
  assert.equal(previewRequirement(fee), "owner_consilium");
});

test("a draft is refused in the order the operator would notice", () => {
  assert.equal(draftProblem(end({ kind: "service" }), fee, "1", "r"), "admin.payments.err.pickSource");
  assert.equal(draftProblem(fee, end({ kind: "external", network: "ton" }), "1", "r"), "admin.payments.err.pickDestination");
  assert.equal(draftProblem(fee, end({ kind: "user" }), "1", "r"), "admin.payments.err.pickDestination");
  assert.equal(draftProblem(fee, fee, "1", "r"), "admin.payments.err.sameEnds");
  assert.equal(draftProblem(end({ kind: "user", id: "a" }), end({ kind: "user", id: "b" }), "1", "r"), null);
  assert.equal(draftProblem(fund, end({ kind: "external", network: "ton", address: "EQ" }), "1", "r"), "admin.payments.err.noRailFromPooled");
  assert.equal(draftProblem(fee, fund, "0", "r"), "admin.payments.err.enterAmount");
  assert.equal(draftProblem(fee, fund, "1.5", "  "), "admin.payments.err.enterReason");
  assert.equal(draftProblem(fee, fund, "1.5", "ё".repeat(251)), "admin.payments.err.reasonTooLong");
  assert.equal(draftProblem(fee, fund, "1.5", "ё".repeat(250)), null);
});

test("the amount is a plain decimal string, not whatever Number would take", () => {
  const ok = (amount: string) => draftProblem(fee, fund, amount, "r");
  for (const amount of ["1", "0.5", " 12.50 ", "1." + "0".repeat(17) + "1"]) assert.equal(ok(amount), null, amount);
  for (const amount of ["", "0", "0.0", "1e5", "0x1a", " .5", "-1", "+1", "1,5", "1." + "0".repeat(19), "Infinity"]) {
    assert.equal(ok(amount), "admin.payments.err.enterAmount", JSON.stringify(amount));
  }
});

test("the wire request names every internal end by id and trims everything", () => {
  const request = toRequest(end({ kind: "service", id: " fee " }), end({ kind: "external", network: " ton ", address: " EQx " }), " 12.50 ", " why ");
  assert.deepEqual(request, {
    source: { kind: "service", id: "fee" },
    destination: { external: { network: "ton", address: "EQx" } },
    amount: "12.50",
    reason: "why",
  });
  assert.deepEqual(toRequest(end({ kind: "user", id: " u1 " }), end({ kind: "service", id: "alpha" }), "1", "r").destination, {
    internal: { kind: "service", id: "alpha" },
  });
});
