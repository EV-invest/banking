// Run with `npm run test` (Node's built-in runner, native type-stripping).
import assert from "node:assert/strict";
import test from "node:test";

import { deriveChecklist } from "./checklist.ts";

const NEW = { level: 0, runningCase: null, total: 0, positions: 0 };

test("a new account starts at verify, with the rest locked", () => {
  assert.deepEqual(deriveChecklist(NEW), { verify: "current", deposit: "locked", invest: "locked", done: 0, complete: false });
});

test("a resumable case is still the verify step's turn — the button says continue", () => {
  assert.equal(deriveChecklist({ ...NEW, runningCase: { resumable: true } }).verify, "current");
});

test("a case with nowhere to return to is in review, and keeps the deposit locked", () => {
  const list = deriveChecklist({ ...NEW, runningCase: { resumable: false } });
  assert.equal(list.verify, "review");
  assert.equal(list.deposit, "locked");
  assert.equal(list.done, 0);
});

test("a verified, empty account is at deposit", () => {
  assert.deepEqual(deriveChecklist({ ...NEW, level: 1 }), { verify: "done", deposit: "current", invest: "locked", done: 1, complete: false });
});

test("a funded account is at invest", () => {
  assert.deepEqual(deriveChecklist({ level: 1, runningCase: null, total: 250, positions: 0 }), { verify: "done", deposit: "done", invest: "current", done: 2, complete: false });
});

test("holding units of any allocation completes the path", () => {
  const list = deriveChecklist({ level: 1, runningCase: null, total: 250, positions: 1 });
  assert.equal(list.complete, true);
  assert.equal(list.done, 3);
});

test("done wins over locked: a tier-0 account credited by hand", () => {
  // The path is the common order, not a rule the hub enforces — an operator can credit an
  // unverified account, and the block must not claim that money is still to come.
  const list = deriveChecklist({ level: 0, runningCase: null, total: 100, positions: 0 });
  assert.equal(list.verify, "current");
  assert.equal(list.deposit, "done");
  assert.equal(list.invest, "current");
});

test("a running case does not un-verify a tier above 0", () => {
  assert.equal(deriveChecklist({ level: 1, runningCase: { resumable: false }, total: 0, positions: 0 }).verify, "done");
});
