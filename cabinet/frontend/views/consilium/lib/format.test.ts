// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// The rule under test is the one that emptied the owners' room's live card (#318): every
// open consilium sat under "Earlier requests" with an Open badge and no tally, while the
// live card said "No request is open". The BFF writes an undecided `decided_at` as the
// STRING "0" — the same shape every unix stamp on this wire has (`shared/lib/unix-stamp.ts`)
// — and `isSettled` read it with `Boolean(...)`, which is true for any non-empty string.
//
// The first assertion is the one that failed against the old code. The counter-cases keep
// the fix honest in the other direction: a real stamp, or a terminal state with no stamp,
// must still read as settled, or the fix would just move every request into "live".
import assert from "node:assert/strict";
import test from "node:test";

import { isSettled } from "./format.ts";

test('an open request whose decided_at is the wire\'s "0" is not settled', () => {
  assert.equal(isSettled("open", "0"), false);
});

test("absence in every other form it arrives in is not settled either", () => {
  assert.equal(isSettled("open", undefined), false);
  assert.equal(isSettled("open", null), false);
  assert.equal(isSettled("open", ""), false);
  assert.equal(isSettled("pending", "0"), false);
});

test("a real decision stamp settles a request whatever its state says", () => {
  assert.equal(isSettled("open", "1789483061"), true);
});

test("a terminal state settles a request even before a stamp is written", () => {
  assert.equal(isSettled("executed", "0"), true);
  assert.equal(isSettled("EXECUTION_FAILED", undefined), true);
  assert.equal(isSettled("expired", null), true);
});

test("a state nobody has met yet is still open — the server, not the page, refuses a vote", () => {
  assert.equal(isSettled("reconciling", "0"), false);
});
