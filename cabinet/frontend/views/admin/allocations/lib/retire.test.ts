// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// The rules the BFF enforces with a 400 and the hub with a refused burn, checked here so
// the form never reaches either: a body always names a holder, the company never gives up
// more than it holds, a live product is only burned out of with an explicit `force`, and
// a retry carries the key of the attempt it retries.
import assert from "node:assert/strict";
import test from "node:test";

import { EMPTY_RETIRE_DRAFT, afterRetired, retireAllowed, retireDraftProblem, retireFingerprint, retireKeyFor, retireUnitsBody, type RetireDraft } from "./retire.ts";

const ann = { kind: "user", userId: "u-1", label: "ann@example.com" } as const;
const company = { kind: "company" } as const;
const draft: RetireDraft = { holder: ann, units: "250", costBasis: "", force: false };
const HELD = "16250.5";

test("a draft with no holder is refused before it becomes a body", () => {
  assert.equal(retireDraftProblem({ ...draft, holder: null }, HELD), "holder");
  assert.equal(retireUnitsBody("arb", "closed", { ...draft, holder: null }, HELD, "k"), null);
  assert.equal(retireUnitsBody("arb", "closed", EMPTY_RETIRE_DRAFT, HELD, "k"), null);
});

test("the body names the holder — a user by id, the company by flag — and carries the key", () => {
  assert.deepEqual(retireUnitsBody("arb", "closed", draft, HELD, "key-1"), { service: "arb", user_id: "u-1", units: "250", idempotency_key: "key-1" });
  assert.deepEqual(retireUnitsBody("arb", "closed", { ...draft, holder: company }, HELD, "key-2"), { service: "arb", company: true, units: "250", idempotency_key: "key-2" });
  // The hub defaults an ABSENT basis to units × NAV; an empty string is a malformed decimal.
  const blank = retireUnitsBody("arb", "closed", { ...draft, costBasis: "  " }, HELD, "k");
  assert.ok(blank);
  assert.equal("cost_basis" in blank, false);
  assert.deepEqual(retireUnitsBody("arb", "closed", { ...draft, units: " 1.5 ", costBasis: "0" }, HELD, "k"), { service: "arb", user_id: "u-1", units: "1.5", idempotency_key: "k", cost_basis: "0" });
});

test("units must be a positive decimal; the company's are capped at what it holds, an investor's are not", () => {
  assert.equal(retireDraftProblem({ ...draft, units: "" }, HELD), "units");
  assert.equal(retireDraftProblem({ ...draft, units: "0" }, HELD), "units");
  assert.equal(retireDraftProblem({ ...draft, units: "-5" }, HELD), "units");
  assert.equal(retireDraftProblem({ ...draft, units: "1e3" }, HELD), "units");
  assert.equal(retireDraftProblem({ ...draft, holder: company, units: "16250.5" }, HELD), null);
  assert.equal(retireDraftProblem({ ...draft, holder: company, units: "16250.500000001" }, HELD), "exceeds");
  assert.equal(retireDraftProblem({ ...draft, holder: company, units: "1" }, "0"), "exceeds");
  assert.equal(retireDraftProblem({ ...draft, holder: company, units: "1" }, undefined), "exceeds");
  // An investor's available units are not on screen — the hub refuses those with a reason.
  assert.equal(retireDraftProblem({ ...draft, units: "99999" }, HELD), null);
  assert.equal(retireDraftProblem({ ...draft, units: "1" }, undefined), null);
  assert.equal(retireUnitsBody("arb", "closed", { ...draft, holder: company, units: "99999" }, HELD, "k"), null);
});

test("a basis need only be a decimal; zero is a legitimate basis", () => {
  assert.equal(retireDraftProblem({ ...draft, costBasis: "0" }, HELD), null);
  assert.equal(retireDraftProblem({ ...draft, costBasis: "12.50" }, HELD), null);
  assert.equal(retireDraftProblem({ ...draft, costBasis: "12,50" }, HELD), "costBasis");
  assert.equal(retireDraftProblem({ ...draft, costBasis: "-1" }, HELD), "costBasis");
});

test("a live product is burned out of only with the override, and the body says so", () => {
  assert.equal(retireAllowed("closed", false), true);
  assert.equal(retireAllowed("open", false), false);
  assert.equal(retireAllowed("draft", false), false);
  assert.equal(retireAllowed("open", true), true);
  assert.equal(retireUnitsBody("arb", "open", draft, HELD, "k"), null);
  assert.equal(retireUnitsBody("arb", "draft", draft, HELD, "k"), null);
  assert.deepEqual(retireUnitsBody("arb", "open", { ...draft, force: true }, HELD, "k"), { service: "arb", user_id: "u-1", units: "250", idempotency_key: "k", force: true });
  // A closed product needs no override, so a stale tick is not sent as one.
  const closed = retireUnitsBody("arb", "closed", { ...draft, force: true }, HELD, "k");
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
  assert.equal(retireKeyFor(first, "arb", { ...draft, holder: company }, mint).key, "key-3");
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
  assert.equal(retireDraftProblem(next, HELD), "units");
  assert.notEqual(retireFingerprint("arb", next), retireFingerprint("arb", draft));
});
