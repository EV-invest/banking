// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// The one check a server read makes on a BFF body before seeding a screen with it. Too loose
// and a screen `.map`s over something that is not an array; too strict and a healthy proto3
// answer — which drops an empty repeated field — is refused.
import assert from "node:assert/strict";
import test from "node:test";

import { hasOptionalLists, isJsonObject } from "./shape.ts";

test("null is not a body", () => {
  assert.equal(hasOptionalLists(null, ["positions"]), false);
});

test("a bare array is not a body, even an array of the right things", () => {
  assert.equal(hasOptionalLists([{ service: "arb" }], ["positions"]), false);
});

test("a named field that is present but not an array refuses the body", () => {
  assert.equal(hasOptionalLists({ positions: { service: "arb" } }, ["positions"]), false);
});

test("a named field that is present as null refuses the body", () => {
  assert.equal(hasOptionalLists({ positions: null }, ["positions"]), false);
});

test("a named field that is absent is an empty list, as proto3 JSON writes one", () => {
  assert.equal(hasOptionalLists({}, ["positions"]), true);
});

test("a named field that is an array is accepted", () => {
  assert.equal(hasOptionalLists({ positions: [] }, ["positions"]), true);
});

test("every named field is checked, not only the first", () => {
  assert.equal(hasOptionalLists({ items: [], positions: "none" }, ["items", "positions"]), false);
});

test("a string or a number is not a body", () => {
  assert.equal(isJsonObject("{}"), false);
  assert.equal(isJsonObject(0), false);
});
