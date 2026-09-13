// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// The socket is a delivery here, not a doorbell, so what a frame is allowed to do to the
// screen is worth pinning: a stale snapshot never paints over a newer one, a heartbeat does
// nothing, and the caller's orders are re-read exactly when their revision moves.
import assert from "node:assert/strict";
import test from "node:test";

import type { BookSocketFrame } from "@/shared/contracts/book";

import { INITIAL_STREAM_STATE, applyFrame, parseFrame, revisionOf } from "./book-frame.ts";

const frame = (revision: string, ordersRevision: string, extra: Partial<BookSocketFrame> = {}): BookSocketFrame => ({
  type: "book",
  snapshot: { service: "arb", revision, bids: [], asks: [] },
  trades: [{ id: "t1", price: "1.02", size: "5", taker_side: "buy" }],
  orders_revision: ordersRevision,
  ...extra,
});

test("only a frame in our shape is a frame", () => {
  assert.equal(parseFrame("not json"), null);
  assert.equal(parseFrame(JSON.stringify({ revision: 3 })), null);
  assert.equal(parseFrame(new ArrayBuffer(4)), null);
  assert.deepEqual(parseFrame(JSON.stringify({ type: "heartbeat", at: "1" })), { type: "heartbeat", at: "1" });
  assert.equal(parseFrame(JSON.stringify(frame("1", "0")))?.type, "book");
});

test("a revision is read as the int64 it is, never as a float", () => {
  assert.equal(revisionOf("9007199254740993"), 9007199254740993n);
  assert.equal(revisionOf(7), 7n);
  assert.equal(revisionOf(undefined), 0n);
  assert.equal(revisionOf("nope"), 0n);
});

test("a heartbeat changes nothing", () => {
  const { state, effects } = applyFrame(INITIAL_STREAM_STATE, { type: "heartbeat", at: "1" });
  assert.equal(state, INITIAL_STREAM_STATE);
  assert.deepEqual(effects, { snapshot: null, trades: null, refetchOrders: false });
});

test("the first frame paints and sets the orders baseline without a refetch", () => {
  const { state, effects } = applyFrame(INITIAL_STREAM_STATE, frame("5", "7"));
  assert.equal(effects.snapshot?.revision, "5");
  assert.equal(effects.trades?.length, 1);
  assert.equal(effects.refetchOrders, false);
  assert.equal(state.bookRevision, 5n);
  assert.equal(state.ordersRevision, 7n);
  assert.equal(state.primed, true);
});

test("the orders are re-read exactly when their revision moves", () => {
  const primed = applyFrame(INITIAL_STREAM_STATE, frame("5", "7")).state;
  // Same revision: the book moved, the caller's orders did not.
  const quiet = applyFrame(primed, frame("6", "7"));
  assert.equal(quiet.effects.refetchOrders, false);
  assert.equal(quiet.effects.snapshot?.revision, "6");
  // Moved: a fill or a cancel landed.
  const moved = applyFrame(quiet.state, frame("7", "9"));
  assert.equal(moved.effects.refetchOrders, true);
  assert.equal(moved.state.ordersRevision, 9n);
});

test("a replayed or re-ordered snapshot never paints over a newer one", () => {
  const at6 = applyFrame(INITIAL_STREAM_STATE, frame("6", "1")).state;
  const stale = applyFrame(at6, frame("4", "1"));
  assert.equal(stale.effects.snapshot, null);
  assert.equal(stale.effects.trades, null);
  assert.equal(stale.state.bookRevision, 6n);
  // The orders revision on a stale frame is still information — it is the caller's own
  // list, not the book, and it only ever moves forward on the hub.
  const staleButMoved = applyFrame(at6, frame("4", "2"));
  assert.equal(staleButMoved.effects.refetchOrders, true);
});

test("a book the hub has not numbered yet is always taken", () => {
  const at6 = applyFrame(INITIAL_STREAM_STATE, frame("6", "1")).state;
  const unnumbered = applyFrame(at6, frame("0", "1"));
  assert.equal(unnumbered.effects.snapshot?.revision, "0");
});
