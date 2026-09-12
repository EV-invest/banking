// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// The preview is a courtesy, but a wrong courtesy is worse than none: an operator told
// "the owners will be asked" who then emails one investor has been misled about who is
// about to read the reason they wrote. These pin the two derivations to the plane's rules.
import assert from "node:assert/strict";
import test from "node:test";

import { EMPTY_END, draftProblem, previewRequirement, previewTier, toRequest, type EndDraft } from "./terms.ts";

const end = (over: Partial<EndDraft>): EndDraft => ({ ...EMPTY_END, ...over });

test("the tier follows the destination alone", () => {
  assert.equal(previewTier(end({ kind: "external", network: "bep20", address: "0xabc" })), "external");
  assert.equal(previewTier(end({ kind: "service", id: "alpha" })), "service");
  assert.equal(previewTier(end({ kind: "user", id: "u1" })), "internal");
  assert.equal(previewTier(end({ kind: "piggybank" })), "internal");
  assert.equal(previewTier(end({ kind: "revenue" })), "internal");
});

test("the requirement follows the source alone", () => {
  assert.equal(previewRequirement(end({ kind: "user", id: "u1" })), "subject_consent");
  for (const kind of ["piggybank", "revenue", "service"] as const) {
    assert.equal(previewRequirement(end({ kind, id: "x" })), "owner_consilium", kind);
  }
});

test("a draft is refused in the order the operator would notice", () => {
  assert.equal(draftProblem(end({ kind: "service" }), end({ kind: "revenue" }), "1", "r"), "admin.payments.err.pickSource");
  assert.equal(draftProblem(end({ kind: "revenue" }), end({ kind: "external", network: "ton" }), "1", "r"), "admin.payments.err.pickDestination");
  assert.equal(draftProblem(end({ kind: "revenue" }), end({ kind: "revenue" }), "1", "r"), "admin.payments.err.sameEnds");
  assert.equal(draftProblem(end({ kind: "user", id: "a" }), end({ kind: "user", id: "b" }), "1", "r"), null);
  assert.equal(
    draftProblem(end({ kind: "piggybank" }), end({ kind: "external", network: "ton", address: "EQ" }), "1", "r"),
    "admin.payments.err.noRailFromPooled",
  );
  assert.equal(draftProblem(end({ kind: "revenue" }), end({ kind: "piggybank" }), "0", "r"), "admin.payments.err.enterAmount");
  assert.equal(draftProblem(end({ kind: "revenue" }), end({ kind: "piggybank" }), "1.5", "  "), "admin.payments.err.enterReason");
  assert.equal(draftProblem(end({ kind: "revenue" }), end({ kind: "piggybank" }), "1.5", "ё".repeat(251)), "admin.payments.err.reasonTooLong");
  assert.equal(draftProblem(end({ kind: "revenue" }), end({ kind: "piggybank" }), "1.5", "ё".repeat(250)), null);
});

test("the wire request drops ids the singletons do not carry and trims everything", () => {
  const request = toRequest(end({ kind: "revenue", id: "stale" }), end({ kind: "external", network: " ton ", address: " EQx " }), " 12.50 ", " why ");
  assert.deepEqual(request, {
    source: { kind: "revenue", id: "" },
    destination: { external: { network: "ton", address: "EQx" } },
    amount: "12.50",
    reason: "why",
  });
  assert.deepEqual(toRequest(end({ kind: "user", id: " u1 " }), end({ kind: "service", id: "alpha" }), "1", "r").destination, {
    internal: { kind: "service", id: "alpha" },
  });
});
