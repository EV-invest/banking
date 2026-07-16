// Run with `npm run test` (Node's built-in runner, native type-stripping).
import assert from "node:assert/strict";
import test from "node:test";

import { tips, type TipEntry } from "./catalog.ts";

const ROLES = new Set(["investor", "operator", "admin", "owner"]);
const entries = Object.entries(tips) as [string, TipEntry][];

test("catalog is non-empty", () => {
  assert.ok(entries.length > 0, "the catalog should have entries");
});

test("every entry has a valid type, title and body", () => {
  for (const [key, e] of entries) {
    assert.ok(e.type === "input" || e.type === "section", `${key}: bad type ${e.type}`);
    assert.ok(e.title.trim().length > 0, `${key}: empty title`);
    assert.ok(e.body.trim().length > 0, `${key}: empty body`);
  }
});

test("copy stays concise and clean", () => {
  for (const [key, e] of entries) {
    assert.ok(e.title.length <= 48, `${key}: title too long (${e.title.length})`);
    assert.ok(e.body.length <= 320, `${key}: body too long (${e.body.length})`);
    assert.ok(!/\s{2,}/.test(e.title + " " + e.body), `${key}: double spaces in copy`);
    assert.equal(e.title, e.title.trim(), `${key}: title has edge whitespace`);
    assert.equal(e.body, e.body.trim(), `${key}: body has edge whitespace`);
  }
});

test("role gates use only known platform roles", () => {
  for (const [key, e] of entries) {
    if (!e.roles) continue;
    assert.ok(e.roles.length > 0, `${key}: empty roles array — omit it instead`);
    for (const r of e.roles) assert.ok(ROLES.has(r), `${key}: unknown role ${r}`);
  }
});

test("admin.* tips are operator-gated; investor tips are not", () => {
  for (const [key, e] of entries) {
    if (key.startsWith("admin.")) {
      assert.ok(e.roles && !e.roles.includes("investor"), `${key}: admin tip must be role-gated`);
    } else {
      assert.equal(e.roles, undefined, `${key}: investor-facing tip should not be role-gated`);
    }
  }
});

test("keys are dot-namespaced by surface", () => {
  for (const [key] of entries) {
    assert.match(key, /^[a-z][a-z0-9-]*(\.[a-z0-9-]+)+$/, `${key}: not a dot.kebab key`);
  }
});
