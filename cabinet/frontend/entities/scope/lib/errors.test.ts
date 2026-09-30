// Run with `npm run test` (Node's built-in runner, native type-stripping).
import assert from "node:assert/strict";
import test from "node:test";

import { scopeErrorKey } from "./errors.ts";

test("a refusal is worded for what the reader was doing", () => {
  assert.equal(scopeErrorKey(403, "list"), "panelAccess.err.forbiddenList");
  assert.equal(scopeErrorKey(403, "grant"), "panelAccess.err.forbidden");
  assert.equal(scopeErrorKey(404, "revoke"), "panelAccess.err.notHolder");
  assert.equal(scopeErrorKey(429, "grant"), "panelAccess.err.tooMany");
  assert.equal(scopeErrorKey(400, "grant"), "panelAccess.err.invalid");
});

test("an unknown address and an unseatable account read alike on grant", () => {
  assert.equal(scopeErrorKey(404, "grant"), "panelAccess.err.cannotGrant");
  assert.equal(scopeErrorKey(412, "grant"), "panelAccess.err.cannotGrant");
});

test("statuses that are not about scopes fall through to the generic surface", () => {
  for (const status of [0, 401, 500, 503]) assert.equal(scopeErrorKey(status, "grant"), null, String(status));
});
