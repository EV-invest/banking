// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// The fund reads a server component makes. Two rules live here and nowhere else: a blank
// service is no request at all (the BFF would only answer it with a 400), and each body is
// checked by a guard before it may seed a screen. `bffRead` is replaced by a recorder, so a
// test sees which path was asked and can put bodies through the guard it was handed.
import assert from "node:assert/strict";
import test, { beforeEach } from "node:test";

import { installModuleHooks } from "../../../shared/__tests__/react-hook-harness.ts";

installModuleHooks({
  "server-only": new URL("./__tests__/server-only.stub.ts", import.meta.url).href,
  "@/shared/api/server/bff": new URL("./__tests__/bff.stub.ts", import.meta.url).href,
});

const { reads } = await import("./__tests__/bff.stub.ts");
const fund = await import("./fund-server.ts");

beforeEach(() => {
  reads.length = 0;
});

/** The guard the one recorded read was handed. */
function onlyGuard(): (body: unknown) => boolean {
  assert.equal(reads.length, 1, `expected one read, got ${reads.length}`);
  return reads[0].accept;
}

test("the allocation list asks the BFF for /api/allocations", async () => {
  await fund.readAllocations();

  assert.deepEqual(
    reads.map((r) => r.path),
    ["/api/allocations"],
  );
});

test("an allocation list without its array is refused, unlike a proto3 list", async () => {
  await fund.readAllocations();

  assert.equal(onlyGuard()({}), false);
});

test("an allocation list whose array is null is refused", async () => {
  await fund.readAllocations();

  assert.equal(onlyGuard()({ allocations: null }), false);
});

test("a bare array is not an allocation list", async () => {
  await fund.readAllocations();

  assert.equal(onlyGuard()([]), false);
});

test("an empty allocation list is accepted", async () => {
  await fund.readAllocations();

  assert.equal(onlyGuard()({ allocations: [] }), true);
});

test("positions may omit their list, as proto3 JSON does for an empty one", async () => {
  await fund.readPositions();

  assert.equal(onlyGuard()({}), true);
  assert.equal(onlyGuard()({ positions: {} }), false);
});

test("redemptions may omit their list but not replace it with an object", async () => {
  await fund.readRedemptions();

  assert.equal(onlyGuard()({}), true);
  assert.equal(onlyGuard()({ redemptions: "none" }), false);
});

test("an allocation detail without a service name is refused", async () => {
  await fund.readAllocation("arb");

  assert.equal(onlyGuard()({}), false);
  assert.equal(onlyGuard()({ service: "arb" }), true);
});

test("an empty service reads no allocation detail and asks nothing", async () => {
  assert.equal(await fund.readAllocation(""), null);
  assert.equal(reads.length, 0);
});

test("a whitespace service reads no NAV and asks nothing", async () => {
  assert.equal(await fund.readFundNav("   "), null);
  assert.equal(reads.length, 0);
});

test("an empty service reads no fee policy and asks nothing", async () => {
  assert.equal(await fund.readFeePolicy(""), null);
  assert.equal(reads.length, 0);
});

test("an empty service reads no accrued fees and asks nothing", async () => {
  assert.equal(await fund.readAccruedFees("\t"), null);
  assert.equal(reads.length, 0);
});

test("a named service is asked for by its encoded name on each per-service read", async () => {
  await fund.readAllocation("a&b");
  await fund.readFundNav("a&b");
  await fund.readFeePolicy("a&b");
  await fund.readAccruedFees("a&b");

  assert.deepEqual(
    reads.map((r) => r.path),
    ["/api/allocations/detail?service=a%26b", "/api/funds/nav?service=a%26b", "/api/funds/fee-policy?service=a%26b", "/api/funds/accrued-fees?service=a%26b"],
  );
});

test("a scalar fund DTO must still be a JSON object", async () => {
  await fund.readFundNav("arb");

  assert.equal(onlyGuard()(null), false);
  assert.equal(onlyGuard()([]), false);
  assert.equal(onlyGuard()({}), true);
});

test("a fee policy body must be a JSON object", async () => {
  await fund.readFeePolicy("arb");

  assert.equal(onlyGuard()([]), false);
  assert.equal(onlyGuard()({}), true);
});

test("an accrued-fees body must be a JSON object", async () => {
  await fund.readAccruedFees("arb");

  assert.equal(onlyGuard()([]), false);
  assert.equal(onlyGuard()({}), true);
});
