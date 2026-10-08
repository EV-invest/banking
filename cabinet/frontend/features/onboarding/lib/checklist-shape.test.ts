// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// The checklist's last shape rides a cookie to the server, which draws its skeleton in that
// shape. The cookie is the reader's to edit, so only the four known shapes may pass; and it
// must outlive the session, as the stage it mirrors does.
import assert from "node:assert/strict";
import test from "node:test";

import { isChecklistShape, SHAPE_COOKIE, shapeCookie } from "./checklist-shape.ts";

test("each of the four shapes the block can take is recognised", () => {
  assert.equal(isChecklistShape("path"), true);
  assert.equal(isChecklistShape("all-set"), true);
  assert.equal(isChecklistShape("line"), true);
  assert.equal(isChecklistShape("none"), true);
});

test("a hand-edited cookie value is not a shape", () => {
  assert.equal(isChecklistShape("complete"), false);
  assert.equal(isChecklistShape("PATH"), false);
  assert.equal(isChecklistShape(" path"), false);
  assert.equal(isChecklistShape(""), false);
});

test("an absent cookie is not a shape", () => {
  assert.equal(isChecklistShape(undefined), false);
  assert.equal(isChecklistShape(null), false);
});

test("the cookie is named for the server to find and lives a year on every path", () => {
  assert.equal(SHAPE_COOKIE, "ev_checklist_shape");
  assert.equal(shapeCookie("all-set"), "ev_checklist_shape=all-set; Path=/; Max-Age=31536000; SameSite=Lax");
});
