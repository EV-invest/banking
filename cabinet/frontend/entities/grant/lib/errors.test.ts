// Run with `npm run test` (Node's built-in runner, native type-stripping).
import assert from "node:assert/strict";
import test from "node:test";

import { grantErrorKey } from "./errors.ts";

test("a refusal is worded for what the reader was doing", () => {
  assert.equal(grantErrorKey(403, "list"), "panelAccess.err.forbiddenList");
  assert.equal(grantErrorKey(403, "grant"), "panelAccess.err.forbidden");
  assert.equal(grantErrorKey(404, "revoke"), "panelAccess.err.notHolder");
  assert.equal(grantErrorKey(429, "grant"), "panelAccess.err.tooMany");
  assert.equal(grantErrorKey(400, "grant"), "panelAccess.err.invalid");
});

test("an unknown address and an unseatable account read alike on grant", () => {
  assert.equal(grantErrorKey(404, "grant"), "panelAccess.err.cannotGrant");
  assert.equal(grantErrorKey(412, "grant"), "panelAccess.err.cannotGrant");
});

test("statuses that are not about grants fall through to the generic surface", () => {
  for (const status of [0, 401, 500, 503]) assert.equal(grantErrorKey(status, "grant"), null, String(status));
});
