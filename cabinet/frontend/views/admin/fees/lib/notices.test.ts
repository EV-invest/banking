// Run with `npm run test` (Node's built-in runner, native type-stripping).
import assert from "node:assert/strict";
import test from "node:test";

import { noticeSummary, type NoticeSource } from "./notices.ts";

const change = (over: Partial<NoticeSource> = {}): NoticeSource => ({
  state: "scheduled",
  notices_waived_by: null,
  notices_waived_at: "0",
  notices_waived_users: [],
  undelivered_notices: 0,
  notices_given_up: 0,
  ...over,
});

test("a scheduled change every holder was told about has nothing to say", () => {
  assert.deepEqual(noticeSummary(change()), { kind: "none" });
});

test("notices the mailer is still trying are queued, not holders who could not be told", () => {
  assert.deepEqual(noticeSummary(change({ undelivered_notices: 2 })), { kind: "queued", queued: 2 });
  assert.deepEqual(noticeSummary(change({ undelivered_notices: 5, notices_given_up: 0 })), { kind: "queued", queued: 5 });
});

test("once the mailer has given up on one, the act is offered over the given-up ones only", () => {
  assert.deepEqual(noticeSummary(change({ undelivered_notices: 3, notices_given_up: 1 })), { kind: "givenUp", givenUp: 1, queued: 2 });
  assert.deepEqual(noticeSummary(change({ undelivered_notices: 3, notices_given_up: 3 })), { kind: "givenUp", givenUp: 3, queued: 0 });
});

test("the given-up figure never exceeds the undelivered one it is a part of", () => {
  assert.deepEqual(noticeSummary(change({ undelivered_notices: 1, notices_given_up: 4 })), { kind: "givenUp", givenUp: 1, queued: 0 });
});

test("a negative given-up figure is a wire fault and reads as none given up", () => {
  assert.deepEqual(noticeSummary(change({ undelivered_notices: 2, notices_given_up: -1 })), { kind: "queued", queued: 2 });
});

test("the count is read only while scheduled — no button on a change that is past that", () => {
  for (const state of ["awaiting_consilium", "active", "superseded", "rejected", "cancelled"]) {
    assert.deepEqual(noticeSummary(change({ state, undelivered_notices: 2, notices_given_up: 2 })), { kind: "none" }, state);
  }
});

test("a waiver outranks the count: responsibility already taken is not offered again", () => {
  const waived = change({
    notices_waived_by: "user-1",
    notices_waived_at: "1750050000",
    notices_waived_users: ["a", "b"],
    undelivered_notices: 2,
    notices_given_up: 2,
  });
  assert.deepEqual(noticeSummary(waived), { kind: "waived", by: "user-1", at: "1750050000", holders: 2 });
});

test("a waiver stays readable in every later state — it is history, not a pending fact", () => {
  const waived = change({ state: "active", notices_waived_by: "user-1", notices_waived_at: "1750050000", notices_waived_users: ["a"] });
  assert.deepEqual(noticeSummary(waived), { kind: "waived", by: "user-1", at: "1750050000", holders: 1 });
});

test("the waiver is told by its moment even when the author is blanked for the reader", () => {
  const waived = change({ notices_waived_at: "1750050000", notices_waived_users: [], undelivered_notices: 1 });
  assert.deepEqual(noticeSummary(waived), { kind: "waived", by: "", at: "1750050000", holders: 0 });
});

test("a missing moment is not a waiver", () => {
  assert.deepEqual(noticeSummary(change({ notices_waived_at: "", undelivered_notices: 1, notices_given_up: 1 })), { kind: "givenUp", givenUp: 1, queued: 0 });
  assert.deepEqual(noticeSummary(change({ notices_waived_at: "0" })), { kind: "none" });
});
