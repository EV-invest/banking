// Run with `npm run test` (Node's built-in runner, native type-stripping).
import assert from "node:assert/strict";
import test from "node:test";

import { pinCapVerdict } from "./pin-cap.ts";

test("nothing to pin while the supply is still zero and nothing is in the relay", () => {
  assert.deepEqual(pinCapVerdict("100000000", "0", "0"), { kind: "nothingIssued" });
  assert.deepEqual(pinCapVerdict("100000000", "0.000", "0"), { kind: "nothingIssued" });
  assert.deepEqual(pinCapVerdict("100000000", undefined, undefined), { kind: "nothingIssued" });
  assert.deepEqual(pinCapVerdict("100000000", "", ""), { kind: "nothingIssued" });
});

test("a queued mint blocks pinning while it is still in the relay", () => {
  // The cap would sit below the real supply the moment the relay posts the 5,000.
  assert.deepEqual(pinCapVerdict("100000000", "16250", "5000"), { kind: "queuedPending", queued: "5000" });
  // The figure is reported as the hub gave it — the UI formats it, the lib does not.
  assert.deepEqual(pinCapVerdict("100000000", "16250", " 5000.5 "), { kind: "queuedPending", queued: "5000.5" });
});

test("a queued mint wins over 'nothing issued' — the open queue is the actionable reason", () => {
  assert.deepEqual(pinCapVerdict("100000000", "0", "13000"), { kind: "queuedPending", queued: "13000" });
  assert.deepEqual(pinCapVerdict("100000000", undefined, "13000"), { kind: "queuedPending", queued: "13000" });
});

test("a queued mint wins over 'already pinned' — the cap that equals settled today is short tomorrow", () => {
  assert.deepEqual(pinCapVerdict("16250", "16250", "1"), { kind: "queuedPending", queued: "1" });
});

test("pinning at settled + queued is rejected: the relay may still park or reject the mint", () => {
  // Had the verdict offered `to: 21250`, a parked mint would leave the cap 5,000 above the
  // real supply — the failure mode the cap exists to prevent. So no `pinnable` at all.
  const verdict = pinCapVerdict("100000000", "16250", "5000");
  assert.equal(verdict.kind, "queuedPending");
  assert.ok(!("to" in verdict));
});

test("an absent or zero queue is the pre-`queued_units` behaviour", () => {
  // Objects cached before the BFF sent the field have no `queued_units` at all.
  assert.deepEqual(pinCapVerdict("100000000", "16250", undefined), { kind: "pinnable", from: "100000000", to: "16250" });
  assert.deepEqual(pinCapVerdict("100000000", "16250", "0"), { kind: "pinnable", from: "100000000", to: "16250" });
  assert.deepEqual(pinCapVerdict("100000000", "16250", "0.000"), { kind: "pinnable", from: "100000000", to: "16250" });
  assert.deepEqual(pinCapVerdict("100000000", "16250", ""), { kind: "pinnable", from: "100000000", to: "16250" });
});

test("nothing to do when the cap already equals what is out, however it is spelled", () => {
  assert.deepEqual(pinCapVerdict("16250", "16250", "0"), { kind: "alreadyPinned" });
  assert.deepEqual(pinCapVerdict("16250.000", "16250", "0"), { kind: "alreadyPinned" });
  assert.deepEqual(pinCapVerdict("16250", "16250.0", undefined), { kind: "alreadyPinned" });
});

test("a cap above the supply pins down to the exact wire figure", () => {
  assert.deepEqual(pinCapVerdict("100000000", "16250", "0"), { kind: "pinnable", from: "100000000", to: "16250" });
  // The outstanding string is sent as the hub gave it — never reformatted for display.
  assert.deepEqual(pinCapVerdict("100000000", "16250.123456789", "0"), { kind: "pinnable", from: "100000000", to: "16250.123456789" });
});

test("a cap below the supply is still pinnable — raising it to the issued figure is a legal move", () => {
  assert.deepEqual(pinCapVerdict("1000", "16250", "0"), { kind: "pinnable", from: "1000", to: "16250" });
});

test("an unreadable cap does not hide the action", () => {
  assert.equal(pinCapVerdict(undefined, "16250", "0").kind, "pinnable");
  assert.equal(pinCapVerdict("", "16250", undefined).kind, "pinnable");
});
