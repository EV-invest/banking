// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// The inbox reads a server component makes. These shapes are hand-written rather than
// generated, and the views map over the lists without a fallback — so, unlike the proto3
// reads, a body without its array is refused instead of seeding a screen that would throw.
// `bffRead` is replaced by a recorder that keeps the path and the guard it was handed.
import assert from "node:assert/strict";
import test, { beforeEach } from "node:test";

import { installModuleHooks } from "../../../shared/__tests__/react-hook-harness.ts";

installModuleHooks({
  "server-only": new URL("../../../shared/api/server/__tests__/server-only.stub.ts", import.meta.url).href,
  "@/shared/api/server/bff": new URL("../../../shared/api/server/__tests__/bff.stub.ts", import.meta.url).href,
});

const { reads } = await import("../../../shared/api/server/__tests__/bff.stub.ts");
const { readNotificationSettings, readNotifications } = await import("./notification-server.ts");

beforeEach(() => {
  reads.length = 0;
});

/** The guard the one recorded read was handed. */
function onlyGuard(): (body: unknown) => boolean {
  assert.equal(reads.length, 1, `expected one read, got ${reads.length}`);
  return reads[0].accept;
}

test("the inbox is read from /api/notifications", async () => {
  await readNotifications();

  assert.deepEqual(
    reads.map((r) => r.path),
    ["/api/notifications"],
  );
});

test("an inbox with its list is accepted", async () => {
  await readNotifications();

  assert.equal(onlyGuard()({ notifications: [] }), true);
});

test("an inbox without a notifications list is refused", async () => {
  await readNotifications();

  assert.equal(onlyGuard()({ unread_count: 0 }), false);
});

test("an inbox whose notifications are not a list is refused", async () => {
  await readNotifications();

  assert.equal(onlyGuard()({ notifications: {} }), false);
});

test("a null body is not an inbox", async () => {
  await readNotifications();

  assert.equal(onlyGuard()(null), false);
});

test("delivery preferences are read from /api/notifications/settings", async () => {
  await readNotificationSettings();

  assert.deepEqual(
    reads.map((r) => r.path),
    ["/api/notifications/settings"],
  );
});

test("delivery preferences with their topics are accepted", async () => {
  await readNotificationSettings();

  assert.equal(onlyGuard()({ in_app_enabled: true, topics: [] }), true);
});

test("delivery preferences without a topics list are refused", async () => {
  await readNotificationSettings();

  assert.equal(onlyGuard()({ in_app_enabled: true }), false);
});

test("a bare array is not a set of delivery preferences", async () => {
  await readNotificationSettings();

  assert.equal(onlyGuard()([]), false);
});
