// Run with `npm run test` (Node's built-in runner, native type-stripping).
import assert from "node:assert/strict";
import test from "node:test";

import { holderName } from "./holder.ts";

test("a holder is named by preference, then legal name, then not at all", () => {
  assert.equal(holderName({ preferred_name: "Val", legal_name: "Valery P" }), "Val");
  assert.equal(holderName({ preferred_name: " ", legal_name: "Valery P" }), "Valery P");
  assert.equal(holderName({ legal_name: "" }), "");
});
