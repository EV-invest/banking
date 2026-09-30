// Run with `npm run test` (Node's built-in runner, native type-stripping).
import assert from "node:assert/strict";
import test from "node:test";

import { servicePanelLink } from "./service-panel.ts";

const URL = "https://sa.evinvest.ltd";
const grant = (role: string, service = "service_arb") => [{ scope: `allocation:${service}`, role }];

test("any role on the Service-Arb scope gets the card; only its admin gets the access link", () => {
  assert.deepEqual(servicePanelLink(URL, "investor", grant("operator")), { href: URL, manage: false });
  assert.deepEqual(servicePanelLink(URL, "investor", grant("admin")), { href: URL, manage: true });
});

test("global admins and owners get both without a grant", () => {
  assert.deepEqual(servicePanelLink(URL, "admin", []), { href: URL, manage: true });
  assert.deepEqual(servicePanelLink(URL, "owner", []), { href: URL, manage: true });
});

test("everyone else gets nothing — a console operator included", () => {
  assert.equal(servicePanelLink(URL, "investor", []), null);
  assert.equal(servicePanelLink(URL, "operator", []), null);
  assert.equal(servicePanelLink(URL, "investor", grant("admin", "real_estate")), null);
  assert.equal(servicePanelLink(URL, undefined, []), null);
});

test("no configured panel URL, no card", () => {
  assert.equal(servicePanelLink(undefined, "owner", []), null);
  assert.equal(servicePanelLink("", "investor", grant("admin")), null);
});
