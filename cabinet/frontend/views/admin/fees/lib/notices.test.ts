// Run with `npm run test` (Node's built-in runner, native type-stripping).
import assert from "node:assert/strict";
import test from "node:test";

import { noticeSummary, type NoticeSource, waiverName, waiverRecord } from "./notices.ts";

const change = (over: Partial<NoticeSource> = {}): NoticeSource => ({
  state: "scheduled",
  notices_waived_by: null,
  notices_waived_by_email: null,
  notices_waived_at: "0",
  notices_waived_users: [],
  undelivered_notices: 0,
  notices_given_up: 0,
  notices_unacknowledged: 0,
  ...over,
});

/** The hub's figures for a change nobody has acknowledged: every given-up notice is uncovered. */
const givenUp = (undelivered: number, givenUp: number, over: Partial<NoticeSource> = {}): NoticeSource =>
  change({ undelivered_notices: undelivered, notices_given_up: givenUp, notices_unacknowledged: givenUp, ...over });

test("a scheduled change every holder was told about has nothing to say", () => {
  assert.deepEqual(noticeSummary(change()), { kind: "none" });
});

test("notices the mailer is still trying are queued, not holders who could not be told", () => {
  assert.deepEqual(noticeSummary(change({ undelivered_notices: 2 })), { kind: "queued", queued: 2 });
  assert.deepEqual(noticeSummary(givenUp(5, 0)), { kind: "queued", queued: 5 });
});

test("once the mailer has given up on one, the act is offered over the given-up ones only", () => {
  assert.deepEqual(noticeSummary(givenUp(3, 1)), { kind: "givenUp", givenUp: 1, queued: 2, waiver: null });
  assert.deepEqual(noticeSummary(givenUp(3, 3)), { kind: "givenUp", givenUp: 3, queued: 0, waiver: null });
});

test("the given-up figure never exceeds the undelivered one it is a part of", () => {
  assert.deepEqual(noticeSummary(givenUp(1, 4)), { kind: "givenUp", givenUp: 1, queued: 0, waiver: null });
});

test("the uncovered figure never exceeds the given-up one it is a part of", () => {
  assert.deepEqual(noticeSummary(givenUp(3, 2, { notices_unacknowledged: 5 })), { kind: "givenUp", givenUp: 2, queued: 1, waiver: null });
});

test("a negative figure is a wire fault and reads as none", () => {
  assert.deepEqual(noticeSummary(givenUp(2, -1)), { kind: "queued", queued: 2 });
  assert.deepEqual(noticeSummary(givenUp(2, 1, { notices_unacknowledged: -1 })), { kind: "queued", queued: 1 });
});

test("the count is read only while scheduled — no button on a change that is past that", () => {
  for (const state of ["awaiting_consilium", "active", "superseded", "rejected", "cancelled"]) {
    assert.deepEqual(noticeSummary(givenUp(2, 2, { state })), { kind: "none" }, state);
  }
});

const record = { notices_waived_by: "user-1", notices_waived_by_email: "ops@example.com", notices_waived_at: "1750050000", notices_waived_users: ["a", "b"] };

test("a waiver that covers every given-up notice stands alone: responsibility is not offered again", () => {
  const waived = change({ ...record, undelivered_notices: 3, notices_given_up: 2, notices_unacknowledged: 0 });
  assert.deepEqual(noticeSummary(waived), { kind: "waived", by: "user-1", email: "ops@example.com", at: "1750050000", holders: 2 });
});

test("holders given up on after the waiver are offered again, over them alone, with the record to extend", () => {
  const later = change({ ...record, undelivered_notices: 4, notices_given_up: 3, notices_unacknowledged: 1 });
  assert.deepEqual(noticeSummary(later), { kind: "givenUp", givenUp: 1, queued: 1, waiver: { by: "user-1", email: "ops@example.com", at: "1750050000", holders: 2 } });
});

test("a waiver stays readable in every later state — it is history, not a pending fact", () => {
  const waived = change({ state: "active", notices_waived_by: "user-1", notices_waived_at: "1750050000", notices_waived_users: ["a"] });
  assert.deepEqual(noticeSummary(waived), { kind: "waived", by: "user-1", email: null, at: "1750050000", holders: 1 });
});

test("the waiver is told by its moment even when the author is blanked for the reader", () => {
  const waived = change({ notices_waived_at: "1750050000", notices_waived_users: [], undelivered_notices: 1 });
  assert.deepEqual(noticeSummary(waived), { kind: "waived", by: "", email: null, at: "1750050000", holders: 0 });
});

test("a missing moment is not a waiver", () => {
  assert.deepEqual(noticeSummary(givenUp(1, 1, { notices_waived_at: "" })), { kind: "givenUp", givenUp: 1, queued: 0, waiver: null });
  assert.deepEqual(noticeSummary(change({ notices_waived_at: "0" })), { kind: "none" });
});

test("the record reads as history whatever the summary says", () => {
  assert.equal(waiverRecord(change()), null);
  const expected = { by: "user-1", email: "ops@example.com", at: "1750050000", holders: 2 };
  assert.deepEqual(waiverRecord(change(record)), expected);
  assert.deepEqual(waiverRecord(change({ ...record, undelivered_notices: 4, notices_given_up: 3, notices_unacknowledged: 1 })), expected);
  assert.deepEqual(waiverRecord(change({ ...record, state: "active" })), expected);
});

test("the author is named by email, by id only when the directory cannot name them, and not at all when blanked", () => {
  assert.equal(waiverName({ by: "user-1", email: "ops@example.com" }), "ops@example.com");
  assert.equal(waiverName({ by: "user-1", email: null }), "user-1");
  assert.equal(waiverName({ by: "user-1", email: "  " }), "user-1");
  assert.equal(waiverName({ by: "", email: null }), null);
  // An email without an id is not a shape the BFF sends, but it is still a name.
  assert.equal(waiverName({ by: "", email: "ops@example.com" }), "ops@example.com");
});
