// Run with `npm run test` (Node's built-in runner, native type-stripping).
import assert from "node:assert/strict";
import test from "node:test";

import { canOpenPanel, isPanelAccessRoute, isServiceId, tenantOf } from "./access.ts";

test("only an allocation with a tenant has a panel", () => {
  assert.equal(tenantOf("service_arb"), "sa");
  assert.equal(tenantOf("real_estate"), null);
  assert.equal(tenantOf("constructor"), null, "an inherited key is no tenant");
});

test("anything held in the namespace opens its panel, and nothing outside it does", () => {
  assert.equal(canOpenPanel(["sa:work:leads:read"], "sa"), true);
  assert.equal(canOpenPanel(["bank:treasury:read", "iam:tenants:grant"], "sa"), false);
  assert.equal(canOpenPanel(["sab:work:leads:read"], "sa"), false);
  assert.equal(canOpenPanel([], "sa"), false);
});

test("only the per-allocation page is open to a delegate", () => {
  assert.equal(isPanelAccessRoute("/admin/allocations/service_arb"), true);
  assert.equal(isPanelAccessRoute("/admin/allocations/service_arb/"), true);
  assert.equal(isPanelAccessRoute("/admin/allocations"), false);
  assert.equal(isPanelAccessRoute("/admin/allocations/service_arb/extra"), false);
  assert.equal(isPanelAccessRoute("/admin/users"), false);
  assert.equal(isPanelAccessRoute("/admin/allocations/Service-Arb"), false);
});

test("a service id is the registry's own shape, nothing looser", () => {
  for (const ok of ["service_arb", "a", "abc123", "x".repeat(64)]) assert.equal(isServiceId(ok), true, ok);
  for (const bad of ["", "Service_Arb", "quy-nhon", "a b", "a%2Fb", "../x", "é", "x".repeat(65)]) assert.equal(isServiceId(bad), false, bad);
});
