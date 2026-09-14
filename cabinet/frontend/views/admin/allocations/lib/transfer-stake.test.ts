// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// The rules the BFF enforces with a 400 and the hub with a refused transfer, checked here
// so the form never reaches either: a body always names a recipient, never more units
// than the company holds, and a retry carries the key of the attempt it retries.
import assert from "node:assert/strict";
import test from "node:test";

import { EMPTY_TRANSFER_DRAFT, afterTransferred, transferDraftProblem, transferFingerprint, transferKeyFor, transferStakeBody, type TransferDraft } from "./transfer-stake.ts";

const ann = { userId: "u-1", label: "ann@example.com" };
const draft: TransferDraft = { recipient: ann, units: "13000", costBasis: "" };
const HELD = "16250.5";

test("a draft with no recipient is refused before it becomes a body", () => {
  assert.equal(transferDraftProblem({ ...draft, recipient: null }, HELD), "recipient");
  assert.equal(transferStakeBody("arb", { ...draft, recipient: null }, HELD, "k"), null);
  assert.equal(transferStakeBody("arb", EMPTY_TRANSFER_DRAFT, HELD, "k"), null);
});

test("the body names the recipient by id and carries the key; no cost basis unless typed", () => {
  assert.deepEqual(transferStakeBody("arb", draft, HELD, "key-1"), { service: "arb", user_id: "u-1", units: "13000", idempotency_key: "key-1" });
  // The hub defaults an ABSENT basis to units × NAV; an empty string is a malformed decimal.
  const blank = transferStakeBody("arb", { ...draft, costBasis: "  " }, HELD, "k");
  assert.ok(blank);
  assert.equal("cost_basis" in blank, false);
  assert.deepEqual(transferStakeBody("arb", { ...draft, units: " 1.5 ", costBasis: "0" }, HELD, "k"), { service: "arb", user_id: "u-1", units: "1.5", idempotency_key: "k", cost_basis: "0" });
});

test("units must be a positive decimal, and at most what the company holds", () => {
  assert.equal(transferDraftProblem({ ...draft, units: "" }, HELD), "units");
  assert.equal(transferDraftProblem({ ...draft, units: "0" }, HELD), "units");
  assert.equal(transferDraftProblem({ ...draft, units: "-5" }, HELD), "units");
  assert.equal(transferDraftProblem({ ...draft, units: "1e3" }, HELD), "units");
  assert.equal(transferDraftProblem({ ...draft, units: "16250.5" }, HELD), null);
  assert.equal(transferDraftProblem({ ...draft, units: "16250.500" }, HELD), null);
  assert.equal(transferDraftProblem({ ...draft, units: "16250.500000001" }, HELD), "exceeds");
  assert.equal(transferDraftProblem({ ...draft, units: "1" }, "0"), "exceeds");
  assert.equal(transferDraftProblem({ ...draft, units: "1" }, undefined), "exceeds");
  assert.equal(transferStakeBody("arb", { ...draft, units: "99999" }, HELD, "k"), null);
});

test("a basis need only be a decimal; zero is a legitimate basis", () => {
  assert.equal(transferDraftProblem({ ...draft, costBasis: "0" }, HELD), null);
  assert.equal(transferDraftProblem({ ...draft, costBasis: "12.50" }, HELD), null);
  assert.equal(transferDraftProblem({ ...draft, costBasis: "12,50" }, HELD), "costBasis");
  assert.equal(transferDraftProblem({ ...draft, costBasis: "-1" }, HELD), "costBasis");
});

test("a retry of the same submission reuses its key; an edited one mints a new key", () => {
  let n = 0;
  const mint = () => `key-${++n}`;
  const first = transferKeyFor(null, "arb", draft, mint);
  assert.equal(first.key, "key-1");
  assert.equal(transferKeyFor(first, "arb", draft, mint), first);
  assert.equal(transferKeyFor(first, "arb", { ...draft, units: " 13000 " }, mint), first);
  // The label is display only — the same person under a changed name is the same hand-over.
  assert.equal(transferKeyFor(first, "arb", { ...draft, recipient: { userId: "u-1", label: "Ann" } }, mint), first);
  assert.equal(transferKeyFor(first, "arb", { ...draft, units: "300" }, mint).key, "key-2");
  assert.equal(transferKeyFor(first, "arb", { ...draft, recipient: { userId: "u-2", label: "bob@example.com" } }, mint).key, "key-3");
  assert.equal(transferKeyFor(first, "other", draft, mint).key, "key-4");
  assert.equal(transferKeyFor(first, "arb", { ...draft, costBasis: "0" }, mint).key, "key-5");
});

test("after a transfer lands the recipient stays and only the figures clear", () => {
  const next = afterTransferred({ ...draft, costBasis: "12.50" });
  assert.deepEqual(next.recipient, ann);
  assert.equal(next.units, "");
  assert.equal(next.costBasis, "");
  assert.equal(transferDraftProblem(next, HELD), "units");
  assert.notEqual(transferFingerprint("arb", next), transferFingerprint("arb", draft));
});
