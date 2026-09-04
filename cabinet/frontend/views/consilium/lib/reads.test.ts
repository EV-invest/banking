// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// The rule under test is the one the owners' room broke in production: a governance read
// that failed must never be narrowed into a statement about the fund. The three reads were
// answering 404 from a stale backend, and the page responded by saying "0 owners" and "There
// is nobody to propose — you are the only owner listed" — two claims about who controls the
// fund, both derived from nothing at all.
//
// Every assertion below is therefore of the same shape: given a read that did not arrive,
// the derivation answers `null` (say nothing) rather than `0` or `[]` (say something false).
// The counter-cases matter just as much — a roster that genuinely arrived empty is still
// allowed to say so, or the fix would just be a page that never speaks.
import assert from "node:assert/strict";
import test from "node:test";

import {
  elevatedWithoutSeat,
  everyReadFailed,
  knownValue,
  mapRead,
  ownerCount,
  readOf,
  removalCandidates,
  rosterState,
  seatedOwnerIds,
  type Read,
} from "./reads.ts";

interface Roster {
  items: { user_id: string }[];
  below_payout_floor: boolean;
}

const NOT_FOUND = new Error("Not found.");

const failed = <T,>(): Read<T> => readOf<T>({ data: undefined, error: NOT_FOUND });
const loading = <T,>(): Read<T> => readOf<T>({ data: undefined, error: null });
const ready = <T,>(value: T): Read<T> => readOf<T>({ data: value, error: null });

test("a read that failed is failed, not empty", () => {
  const roster = failed<Roster>();
  assert.deepEqual(roster, { status: "failed", error: NOT_FOUND });
  assert.equal(knownValue(roster), null);
});

test("a read still in flight is not an empty read either", () => {
  const roster = loading<Roster>();
  assert.equal(roster.status, "loading");
  assert.equal(knownValue(roster), null);
});

test("a failed refresh keeps the value already on screen", () => {
  // The cache holds the stale value beside the error on purpose; a poll that timed out must
  // not blank a roster the reader is looking at.
  const roster = readOf<Roster>({ data: { items: [{ user_id: "a" }], below_payout_floor: false }, error: NOT_FOUND });
  assert.equal(roster.status, "ready");
  assert.equal(ownerCount(roster), 1);
});

test("a failed roster has no owner count — and specifically not a count of zero", () => {
  // This is the "0 владельцев" in the card header, character for character.
  assert.equal(ownerCount(failed<Roster>()), null);
  assert.equal(ownerCount(loading<Roster>()), null);
  assert.notEqual(ownerCount(failed<Roster>()), 0);
});

test("a roster that really arrived empty is still allowed to say so", () => {
  assert.equal(ownerCount(ready<Roster>({ items: [], below_payout_floor: true })), 0);
});

test("an omitted list on a read that arrived is empty, not unknown", () => {
  // proto3 JSON drops an empty repeated field, so the roster comes back with no `items` at
  // all. That is the fund saying "none" and must not be confused with a read that failed —
  // the whole point of the distinction cuts both ways.
  const roster = ready<Roster>({ below_payout_floor: true } as Roster);
  assert.equal(ownerCount(roster), 0);
  assert.deepEqual(removalCandidates(roster, "me"), []);
});

test("a failed roster yields no candidate list, so the page cannot claim you are alone", () => {
  // `null`, not `[]`: an empty candidate list is what renders "There is nobody to propose".
  assert.equal(removalCandidates(failed<Roster>(), "me"), null);
  assert.equal(removalCandidates(loading<Roster>(), "me"), null);
});

test("a roster that arrived with only you in it may say there is nobody to propose", () => {
  const roster = ready<Roster>({ items: [{ user_id: "me" }], below_payout_floor: true });
  assert.deepEqual(removalCandidates(roster, "me"), []);
});

test("candidates never include the reader", () => {
  const roster = ready<Roster>({ items: [{ user_id: "me" }, { user_id: "you" }], below_payout_floor: false });
  assert.deepEqual(removalCandidates(roster, "me"), [{ user_id: "you" }]);
});

test("a derivation over a read that never arrived is not run at all", () => {
  let ran = false;
  const projected = mapRead(failed<Roster>(), (value) => {
    ran = true;
    return value.items.length;
  });
  assert.equal(ran, false);
  assert.equal(projected.status, "failed");
  assert.equal(knownValue(projected), null);
});

test("one page-level failure only when every read failed", () => {
  assert.equal(everyReadFailed([failed(), failed(), failed()]), true);
});

// ── an empty roster is a real state, not a fault ──────────────────────────────
//
// These pin the second half of the same rule. The first half was "do not turn a failed
// read into a fact"; this one is "do not turn a fact into a fault". An empty roster used to
// be treated as impossible, and the page told the reader to report it — which is exactly
// wrong on a fund whose only elevated accounts are break-glass ones promoted from
// `ADMIN_SUBJECTS`, where nobody holds a seat and the roster is telling the truth.

test("an empty roster that arrived is unseated, not failed", () => {
  assert.equal(rosterState(ready<Roster>({ items: [], below_payout_floor: true })), "unseated");
  assert.equal(rosterState(ready<Roster>({ below_payout_floor: true } as Roster)), "unseated");
});

test("a roster that did not arrive is never reported as unseated", () => {
  // The whole point of keeping these apart: "nobody holds a seat" and "we could not find
  // out" get different copy, and only one of them is worth reporting as a fault.
  assert.equal(rosterState(failed<Roster>()), "failed");
  assert.equal(rosterState(loading<Roster>()), "loading");
});

test("a roster with owners in it is seated", () => {
  assert.equal(rosterState(ready<Roster>({ items: [{ user_id: "a" }], below_payout_floor: true })), "seated");
});

// ── the two screens must not contradict each other ────────────────────────────

test("seated ids are known only from a roster that arrived", () => {
  assert.equal(seatedOwnerIds(failed<Roster>()), null);
  assert.equal(seatedOwnerIds(loading<Roster>()), null);
  assert.deepEqual([...seatedOwnerIds(ready<Roster>({ items: [{ user_id: "a" }], below_payout_floor: true }))!], ["a"]);
  // An omitted list on a read that arrived is the fund saying "none", so the answer is an
  // empty set — which is a real answer, unlike `null`.
  assert.deepEqual([...seatedOwnerIds(ready<Roster>({ below_payout_floor: true } as Roster))!], []);
});

test("an owner-labelled account absent from the roster is marked as holding no seat", () => {
  // The prod contradiction, exactly: three accounts render as Owner because concierge
  // promotes every ADMIN_SUBJECTS subject with a non-persisted `effective_role`, while the
  // roster — read from the persisted role on purpose — holds none of them.
  const seated = new Set(["seated-owner"]);
  assert.equal(elevatedWithoutSeat({ user_id: "break-glass", role: "owner" }, seated), true);
  assert.equal(elevatedWithoutSeat({ user_id: "seated-owner", role: "owner" }, seated), false);
});

test("only the owner role is marked — an admin with no seat is not a contradiction", () => {
  const seated = new Set<string>();
  for (const role of ["investor", "operator", "admin", "", null, undefined]) {
    assert.equal(elevatedWithoutSeat({ user_id: "u", role }, seated), false, `role: ${String(role)}`);
  }
  // The wire's casing is not the screen's, and a capitalised role must not slip the check.
  assert.equal(elevatedWithoutSeat({ user_id: "u", role: "OWNER" }, seated), true);
  assert.equal(elevatedWithoutSeat({ user_id: "u", role: " owner " }, seated), true);
});

test("with no roster to compare against, nothing is marked at all", () => {
  // `/api/owners` is owner-only, so an operator on the users screen gets a 403 there. The
  // degradation has to be silence: marking on an unknown roster would accuse a real owner
  // of holding no seat, which is the same lie as "0 owners" aimed at a different screen.
  assert.equal(elevatedWithoutSeat({ user_id: "anyone", role: "owner" }, null), false);
  assert.equal(elevatedWithoutSeat({ user_id: "anyone", role: "owner" }, seatedOwnerIds(failed<Roster>())), false);
});

test("a partial failure stays partial — it is information, not one outage", () => {
  // "The roster loaded but the payouts did not" has to survive as three cards in three
  // different states; collapsing it into a page-level banner would erase which half is
  // trustworthy.
  assert.equal(everyReadFailed([ready({}), failed(), failed()]), false);
  assert.equal(everyReadFailed([loading(), failed(), failed()]), false);
  assert.equal(everyReadFailed([]), false);
});
