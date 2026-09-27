// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// The rules the BFF enforces with a 400 and the hub with a refused burn, checked here so
// the form never reaches either: a body always names a person, a live product is only
// burned out of with an explicit `force`, and a retry carries the key of the attempt it
// retries.
import assert from "node:assert/strict";
import test from "node:test";

import { EMPTY_RETIRE_DRAFT, afterRetired, retireAllowed, retireDraftProblem, retireFingerprint, retireKeyFor, retireUnitsBody, type RetireDraft } from "./retire.ts";

const ann = { userId: "u-1", label: "ann@example.com" } as const;
const bob = { userId: "u-2", label: "bob@example.com" } as const;
const draft: RetireDraft = { holder: ann, units: "250", costBasis: "", force: false };

test("a draft with no holder is refused before it becomes a body", () => {
  assert.equal(retireDraftProblem({ ...draft, holder: null }), "holder");
  assert.equal(retireUnitsBody("arb", "closed", { ...draft, holder: null }, "k"), null);
  assert.equal(retireUnitsBody("arb", "closed", EMPTY_RETIRE_DRAFT, "k"), null);
});

test("the body names the person by id and carries the key — there is no company arm", () => {
  const body = retireUnitsBody("arb", "closed", draft, "key-1");
  assert.deepEqual(body, { service: "arb", user_id: "u-1", units: "250", idempotency_key: "key-1" });
  assert.equal(body && "company" in body, false);
  // The hub defaults an ABSENT basis to units × NAV; an empty string is a malformed decimal.
  const blank = retireUnitsBody("arb", "closed", { ...draft, costBasis: "  " }, "k");
  assert.ok(blank);
  assert.equal("cost_basis" in blank, false);
  assert.deepEqual(retireUnitsBody("arb", "closed", { ...draft, units: " 1.5 ", costBasis: "0" }, "k"), { service: "arb", user_id: "u-1", units: "1.5", idempotency_key: "k", cost_basis: "0" });
});

test("units must be a positive decimal; what the person has available is the hub's to refuse", () => {
  assert.equal(retireDraftProblem({ ...draft, units: "" }), "units");
  assert.equal(retireDraftProblem({ ...draft, units: "0" }), "units");
  assert.equal(retireDraftProblem({ ...draft, units: "-5" }), "units");
  assert.equal(retireDraftProblem({ ...draft, units: "1e3" }), "units");
  // A person's available units are not on screen — the hub refuses those with a reason.
  assert.equal(retireDraftProblem({ ...draft, units: "99999" }), null);
});

test("a basis need only be a decimal; zero is a legitimate basis", () => {
  assert.equal(retireDraftProblem({ ...draft, costBasis: "0" }), null);
  assert.equal(retireDraftProblem({ ...draft, costBasis: "12.50" }), null);
  assert.equal(retireDraftProblem({ ...draft, costBasis: "12,50" }), "costBasis");
  assert.equal(retireDraftProblem({ ...draft, costBasis: "-1" }), "costBasis");
});

test("a live product is burned out of only with the override, and the body says so", () => {
  assert.equal(retireAllowed("closed", false), true);
  assert.equal(retireAllowed("open", false), false);
  assert.equal(retireAllowed("draft", false), false);
  assert.equal(retireAllowed("open", true), true);
  assert.equal(retireUnitsBody("arb", "open", draft, "k"), null);
  assert.equal(retireUnitsBody("arb", "draft", draft, "k"), null);
  assert.deepEqual(retireUnitsBody("arb", "open", { ...draft, force: true }, "k"), { service: "arb", user_id: "u-1", units: "250", idempotency_key: "k", force: true });
  // A closed product needs no override, so a stale tick is not sent as one.
  const closed = retireUnitsBody("arb", "closed", { ...draft, force: true }, "k");
  assert.ok(closed);
  assert.equal("force" in closed, false);
});

test("a retry of the same submission reuses its key; an edited one mints a new key", () => {
  let n = 0;
  const mint = () => `key-${++n}`;
  const first = retireKeyFor(null, "arb", draft, mint);
  assert.equal(first.key, "key-1");
  assert.equal(retireKeyFor(first, "arb", draft, mint), first);
  assert.equal(retireKeyFor(first, "arb", { ...draft, units: " 250 " }, mint), first);
  // The label is display only — the same person under a changed name is the same burn.
  assert.equal(retireKeyFor(first, "arb", { ...draft, holder: { ...ann, label: "Ann" } }, mint), first);
  assert.equal(retireKeyFor(first, "arb", { ...draft, units: "300" }, mint).key, "key-2");
  assert.equal(retireKeyFor(first, "arb", { ...draft, holder: bob }, mint).key, "key-3");
  assert.equal(retireKeyFor(first, "other", draft, mint).key, "key-4");
  assert.equal(retireKeyFor(first, "arb", { ...draft, costBasis: "0" }, mint).key, "key-5");
  // Forcing is a different decision from not forcing, not a retry of it.
  assert.equal(retireKeyFor(first, "arb", { ...draft, force: true }, mint).key, "key-6");
});

test("after a retirement lands the holder stays; the figures and the override clear", () => {
  const next = afterRetired({ ...draft, costBasis: "12.50", force: true });
  assert.deepEqual(next.holder, ann);
  assert.equal(next.units, "");
  assert.equal(next.costBasis, "");
  assert.equal(next.force, false);
  assert.equal(retireDraftProblem(next), "units");
  assert.notEqual(retireFingerprint("arb", next), retireFingerprint("arb", draft));
});
