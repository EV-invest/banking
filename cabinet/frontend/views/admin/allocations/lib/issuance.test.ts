// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// Two rules the BFF enforces with a 400 and the hub with a refused mint, checked here so
// the form never reaches either: a body always names a person, and a retry carries the
// key of the attempt it retries.
import assert from "node:assert/strict";
import test from "node:test";

import { EMPTY_ISSUE_DRAFT, afterIssued, issueDraftProblem, issueUnitsBody, submissionFingerprint, submissionKeyFor, type IssueDraft } from "./issuance.ts";

const investor: IssueDraft = { holder: { userId: "u-1", label: "ann@example.com" }, units: "250", costBasis: "" };
const other: IssueDraft = { holder: { userId: "u-2", label: "bob@example.com" }, units: "1000.5", costBasis: "0" };

test("a draft with no holder is refused before it becomes a body", () => {
  assert.equal(issueDraftProblem({ ...investor, holder: null }), "holder");
  assert.equal(issueUnitsBody("arb", { ...investor, holder: null }, "k"), null);
  assert.equal(issueUnitsBody("arb", EMPTY_ISSUE_DRAFT, "k"), null);
});

test("the body names the person by id and carries the key — there is no company arm", () => {
  const body = issueUnitsBody("arb", investor, "key-1");
  assert.deepEqual(body, { service: "arb", user_id: "u-1", units: "250", idempotency_key: "key-1" });
  assert.equal(body && "company" in body, false);
  assert.deepEqual(issueUnitsBody("arb", other, "key-2"), { service: "arb", user_id: "u-2", units: "1000.5", idempotency_key: "key-2", cost_basis: "0" });
});

test("an empty cost basis is absent from the body, not an empty string", () => {
  // The hub defaults an ABSENT basis to units × NAV; an empty string is a malformed decimal.
  const body = issueUnitsBody("arb", { ...investor, costBasis: "  " }, "k");
  assert.ok(body);
  assert.equal("cost_basis" in body, false);
});

test("units must be a positive decimal; a basis need only be a decimal", () => {
  assert.equal(issueDraftProblem({ ...investor, units: "" }), "units");
  assert.equal(issueDraftProblem({ ...investor, units: "0" }), "units");
  assert.equal(issueDraftProblem({ ...investor, units: "0.000" }), "units");
  assert.equal(issueDraftProblem({ ...investor, units: "-5" }), "units");
  assert.equal(issueDraftProblem({ ...investor, units: "1e3" }), "units");
  assert.equal(issueDraftProblem({ ...investor, units: "0.001" }), null);
  assert.equal(issueDraftProblem({ ...investor, costBasis: "0" }), null);
  assert.equal(issueDraftProblem({ ...investor, costBasis: "12.50" }), null);
  assert.equal(issueDraftProblem({ ...investor, costBasis: "12,50" }), "costBasis");
  assert.equal(issueDraftProblem({ ...investor, costBasis: "-1" }), "costBasis");
});

test("a retry of the same submission reuses its key; an edited one mints a new key", () => {
  let n = 0;
  const mint = () => `key-${++n}`;
  const first = submissionKeyFor(null, "arb", investor, mint);
  assert.equal(first.key, "key-1");
  // Same body after a timeout: the key the hub may already have seen is sent again.
  assert.equal(submissionKeyFor(first, "arb", investor, mint), first);
  // Whitespace is not a different decision, and neither is the holder's display label.
  assert.equal(submissionKeyFor(first, "arb", { ...investor, units: " 250 " }, mint), first);
  assert.equal(submissionKeyFor(first, "arb", { ...investor, holder: { userId: "u-1", label: "Ann" } }, mint), first);
  // A different figure, holder or product is a different decision.
  assert.equal(submissionKeyFor(first, "arb", { ...investor, units: "300" }, mint).key, "key-2");
  assert.equal(submissionKeyFor(first, "arb", other, mint).key, "key-3");
  assert.equal(submissionKeyFor(first, "other", investor, mint).key, "key-4");
});

test("after a mint lands the holder stays and only the figures clear", () => {
  const next = afterIssued({ ...investor, units: "250", costBasis: "12.50" });
  assert.deepEqual(next.holder, investor.holder);
  assert.equal(next.units, "");
  assert.equal(next.costBasis, "");
  assert.equal(issueDraftProblem(next), "units");
});

test("a second issue to the kept holder is a new decision with a new key", () => {
  let n = 0;
  const mint = () => `key-${++n}`;
  const first = submissionKeyFor(null, "arb", investor, mint);
  // The cleared draft is a different body, so even an un-retired key would not be reused.
  assert.notEqual(submissionFingerprint("arb", afterIssued(investor)), first.fingerprint);
  // And the form retires the key on success, so the very same figure typed again — the
  // same holder issued 250 twice on purpose — is sent under a fresh key rather than
  // silently de-duplicated by the hub.
  const again = submissionKeyFor(null, "arb", investor, mint);
  assert.notEqual(again.key, first.key);
  assert.equal(again.fingerprint, first.fingerprint);
});
