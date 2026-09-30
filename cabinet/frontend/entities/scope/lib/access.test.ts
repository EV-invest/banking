// Run with `npm run test` (Node's built-in runner, native type-stripping).
import assert from "node:assert/strict";
import test from "node:test";

import { canManagePanelAccess, canOpenPanel, canRevokeHolder, grantableRoles, isPanelAccessRoute, isServiceId, scopeRoleOf } from "./access.ts";

const SA = "service_arb";
const saAdmin = [{ scope: "allocation:service_arb", role: "admin" }];
const saOperator = [{ scope: "allocation:service_arb", role: "operator" }];
const elsewhereAdmin = [{ scope: "allocation:real_estate", role: "admin" }];

test("global admins and owners manage every scope without holding a grant", () => {
  for (const role of ["admin", "owner"]) {
    assert.equal(canManagePanelAccess(role, [], SA), true, role);
    assert.equal(canOpenPanel(role, undefined, SA), true, role);
  }
});

test("a global operator or investor gets nothing from the global role alone", () => {
  for (const role of ["operator", "investor", undefined]) {
    assert.equal(canManagePanelAccess(role, [], SA), false, String(role));
    assert.equal(canOpenPanel(role, [], SA), false, String(role));
  }
});

test("the scope's own admin manages it; its operator only opens the panel", () => {
  assert.equal(canManagePanelAccess("investor", saAdmin, SA), true);
  assert.equal(canManagePanelAccess("investor", saOperator, SA), false);
  assert.equal(canOpenPanel("investor", saOperator, SA), true);
  assert.equal(canOpenPanel("investor", saAdmin, SA), true);
});

test("a grant on another allocation opens nothing here", () => {
  assert.equal(canManagePanelAccess("investor", elsewhereAdmin, SA), false);
  assert.equal(canOpenPanel("investor", elsewhereAdmin, SA), false);
});

test("a retired role on the wire is no role", () => {
  assert.equal(scopeRoleOf([{ scope: "allocation:service_arb", role: "viewer" }], SA), null);
  assert.equal(canOpenPanel("investor", [{ scope: "allocation:service_arb", role: "viewer" }], SA), false);
});

test("only a global admin is offered the admin role, and may revoke an admin", () => {
  assert.deepEqual(grantableRoles("admin"), ["operator", "admin"]);
  assert.deepEqual(grantableRoles("owner"), ["operator", "admin"]);
  assert.deepEqual(grantableRoles("investor"), ["operator"]);
  assert.equal(canRevokeHolder("owner", "admin"), true);
  assert.equal(canRevokeHolder("investor", "admin"), false);
  assert.equal(canRevokeHolder("investor", "operator"), true);
});

test("only the per-allocation page is open to a scope admin", () => {
  assert.equal(isPanelAccessRoute("/admin/allocations/service_arb"), true);
  assert.equal(isPanelAccessRoute("/admin/allocations/service_arb/"), true);
  assert.equal(isPanelAccessRoute("/admin/allocations"), false);
  assert.equal(isPanelAccessRoute("/admin/allocations/service_arb/extra"), false);
  assert.equal(isPanelAccessRoute("/admin/users"), false);
  assert.equal(isPanelAccessRoute("/admin/allocations/Service-Arb"), false);
});

test("a service id is the plane's own shape, nothing looser", () => {
  for (const ok of ["service_arb", "a", "abc123", "x".repeat(64)]) assert.equal(isServiceId(ok), true, ok);
  for (const bad of ["", "Service_Arb", "quy-nhon", "a b", "a%2Fb", "../x", "é", "x".repeat(65)]) assert.equal(isServiceId(bad), false, bad);
});
