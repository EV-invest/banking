// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// The profile read a server component makes seeds Settings, Profile and the account chip.
// Every profile field is optional, so the guard refuses only a body that is not an object
// at all. `bffRead` is replaced by a recorder that keeps the guard it was handed.
import assert from "node:assert/strict";
import test, { beforeEach } from "node:test";

import { installModuleHooks } from "../../../shared/__tests__/react-hook-harness.ts";

installModuleHooks({
  "server-only": new URL("../../../shared/api/server/__tests__/server-only.stub.ts", import.meta.url).href,
  "@/shared/api/server/bff": new URL("../../../shared/api/server/__tests__/bff.stub.ts", import.meta.url).href,
});

const { reads } = await import("../../../shared/api/server/__tests__/bff.stub.ts");
const { readProfile } = await import("./profile-server.ts");

beforeEach(() => {
  reads.length = 0;
});

/** The guard the one recorded read was handed. */
async function profileGuard(): Promise<(body: unknown) => boolean> {
  await readProfile();
  assert.equal(reads.length, 1, `expected one read, got ${reads.length}`);
  return reads[0].accept;
}

test("the profile is read from /api/users", async () => {
  await readProfile();

  assert.deepEqual(
    reads.map((r) => r.path),
    ["/api/users"],
  );
});

test("a profile with no fields is accepted, as proto3 JSON may send it", async () => {
  assert.equal((await profileGuard())({}), true);
});

test("a null body is not a profile", async () => {
  assert.equal((await profileGuard())(null), false);
});

test("a bare array is not a profile", async () => {
  assert.equal((await profileGuard())([]), false);
});

test("a string body is not a profile", async () => {
  assert.equal((await profileGuard())("Ada"), false);
});
