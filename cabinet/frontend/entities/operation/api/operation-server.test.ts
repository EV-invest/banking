// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// The timeline read a server component makes must ask for the same path the browser read
// does (`operation-client.ts`), or its answer seeds the resource under the wrong key: the
// full list for Operations and Profile, a `?limit=` page for Home's preview. `bffRead` is
// replaced by a recorder, so a test sees which path was asked and the guard it was handed.
import assert from "node:assert/strict";
import test, { beforeEach } from "node:test";

import { installModuleHooks } from "../../../shared/__tests__/react-hook-harness.ts";

installModuleHooks({
  "server-only": new URL("../../../shared/api/server/__tests__/server-only.stub.ts", import.meta.url).href,
  "@/shared/api/server/bff": new URL("../../../shared/api/server/__tests__/bff.stub.ts", import.meta.url).href,
});

const { reads } = await import("../../../shared/api/server/__tests__/bff.stub.ts");
const { readOperations } = await import("./operation-server.ts");

beforeEach(() => {
  reads.length = 0;
});

test("the full timeline is read from /api/operations with no limit", async () => {
  await readOperations();

  assert.deepEqual(
    reads.map((r) => r.path),
    ["/api/operations"],
  );
});

test("a limited timeline asks for that many rows", async () => {
  await readOperations(5);

  assert.deepEqual(
    reads.map((r) => r.path),
    ["/api/operations?limit=5"],
  );
});

test("a timeline whose operations are not a list is refused", async () => {
  await readOperations();

  assert.equal(reads[0].accept({ operations: {} }), false);
});

test("a timeline may omit its list, as proto3 JSON does for an empty one", async () => {
  await readOperations();

  assert.equal(reads[0].accept({}), true);
});
