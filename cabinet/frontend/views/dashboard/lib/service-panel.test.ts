// Run with `npm run test` (Node's built-in runner, native type-stripping).
import assert from "node:assert/strict";
import test from "node:test";

import { servicePanelLink } from "./service-panel.ts";

const URL = "https://sa.evinvest.ltd";

test("anything held in the Service-Arb namespace gets the card; managing it adds the access link", () => {
  assert.deepEqual(servicePanelLink(URL, ["sa:work:leads:read"], false), { href: URL, manage: false });
  assert.deepEqual(servicePanelLink(URL, ["sa:work:leads:read"], true), { href: URL, manage: true });
});

test("nothing in the namespace, no card — whatever else the reader holds", () => {
  assert.equal(servicePanelLink(URL, [], false), null);
  assert.equal(servicePanelLink(URL, ["bank:treasury:read", "iam:tenants:grant"], true), null);
});

test("no configured panel URL, no card", () => {
  assert.equal(servicePanelLink(undefined, ["sa:work:leads:read"], true), null);
  assert.equal(servicePanelLink("", ["sa:work:leads:read"], true), null);
});
