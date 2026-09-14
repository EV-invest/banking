// Run with `npm run test` (Node's built-in runner, native type-stripping).
import assert from "node:assert/strict";
import test from "node:test";

import { pinCapVerdict } from "./pin-cap.ts";

test("nothing to pin while the supply is still zero — a queued mint is not out yet", () => {
  assert.deepEqual(pinCapVerdict("100000000", "0"), { kind: "nothingIssued" });
  assert.deepEqual(pinCapVerdict("100000000", "0.000"), { kind: "nothingIssued" });
  assert.deepEqual(pinCapVerdict("100000000", undefined), { kind: "nothingIssued" });
  assert.deepEqual(pinCapVerdict("100000000", ""), { kind: "nothingIssued" });
});

test("nothing to do when the cap already equals what is out, however it is spelled", () => {
  assert.deepEqual(pinCapVerdict("16250", "16250"), { kind: "alreadyPinned" });
  assert.deepEqual(pinCapVerdict("16250.000", "16250"), { kind: "alreadyPinned" });
  assert.deepEqual(pinCapVerdict("16250", "16250.0"), { kind: "alreadyPinned" });
});

test("a cap above the supply pins down to the exact wire figure", () => {
  assert.deepEqual(pinCapVerdict("100000000", "16250"), { kind: "pinnable", from: "100000000", to: "16250" });
  // The outstanding string is sent as the hub gave it — never reformatted for display.
  assert.deepEqual(pinCapVerdict("100000000", "16250.123456789"), { kind: "pinnable", from: "100000000", to: "16250.123456789" });
});

test("a cap below the supply is still pinnable — raising it to the issued figure is a legal move", () => {
  assert.deepEqual(pinCapVerdict("1000", "16250"), { kind: "pinnable", from: "1000", to: "16250" });
});

test("an unreadable cap does not hide the action", () => {
  assert.equal(pinCapVerdict(undefined, "16250").kind, "pinnable");
  assert.equal(pinCapVerdict("", "16250").kind, "pinnable");
});
