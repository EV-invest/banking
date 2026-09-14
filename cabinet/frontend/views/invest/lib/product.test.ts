// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// The product page and the terminal resolve their product from the detail read, not the
// catalog: a `hidden` product an operator granted this caller is absent from the open
// list, and resolving from the list alone locked such a holder out of a form the hub
// would have accepted (#251).
import assert from "node:assert/strict";
import test from "node:test";

import type { Allocation, Position } from "../../../shared/contracts/index.ts";

import { blockedReasonKey, isClosed, isLocked, selectProduct, type ProductReads } from "./product.ts";

const allocation = (over: Partial<Allocation> = {}): Allocation => ({
  service: "arb",
  title: "Arbitrage",
  summary: "",
  state: "open",
  created_at: "0",
  updated_at: "0",
  unit_cap: "1000",
  access: "hidden",
  caller_access: "invest",
  ...over,
});

const position: Position = { service: "arb", units: "10", nav: "1", value: "10", cost_basis: "10", pnl: "0" };

const notFound = Object.assign(new Error("not found"), { status: 404 });

const reads = (over: Partial<ProductReads> = {}): ProductReads => ({
  detail: { data: undefined, error: null, isLoading: false },
  catalog: [],
  positions: { data: [], isLoading: false },
  ...over,
});

test("a hidden product with a grant resolves from the detail and is not locked", () => {
  const product = selectProduct("arb", reads({ detail: { data: allocation(), error: null, isLoading: false } }));
  assert.ok(product);
  assert.equal(product.allocation?.caller_access, "invest");
  assert.equal(isLocked(product), false);
  assert.equal(isClosed(product), false);
  assert.equal(blockedReasonKey(product, null), null);
});

test("a view-only caller is locked, whether the detail or the catalog says so", () => {
  const detail = selectProduct("arb", reads({ detail: { data: allocation({ caller_access: "view" }), error: null, isLoading: false } }));
  assert.ok(detail);
  assert.equal(isLocked(detail), true);
  assert.equal(blockedReasonKey(detail, null), "invest.blocked.locked");
  // First frame: the detail is in flight and the catalog paints the row it listed.
  const fromCatalog = selectProduct("arb", reads({ detail: { data: undefined, error: null, isLoading: true }, catalog: [allocation({ access: "view", caller_access: "view" })] }));
  assert.ok(fromCatalog);
  assert.equal(isLocked(fromCatalog), true);
});

test("a 404 from the detail is not registered — unless the caller still holds units in it", () => {
  assert.equal(selectProduct("arb", reads({ detail: { data: undefined, error: notFound, isLoading: false } })), null);
  const held = selectProduct("arb", reads({ detail: { data: undefined, error: notFound, isLoading: false }, positions: { data: [position], isLoading: false } }));
  assert.ok(held);
  assert.equal(held.allocation, null);
  assert.equal(held.position, position);
  assert.equal(isClosed(held), true);
  assert.equal(blockedReasonKey(held, null), "invest.blocked.closed");
});

test("still loading is undefined, not null — the not-found state must not flash", () => {
  assert.equal(selectProduct("arb", reads({ detail: { data: undefined, error: null, isLoading: true }, catalog: undefined })), undefined);
  assert.equal(selectProduct("arb", reads({ detail: { data: undefined, error: null, isLoading: true } })), undefined);
  assert.equal(selectProduct("arb", reads({ detail: { data: allocation(), error: null, isLoading: false }, positions: { data: undefined, isLoading: true } })), undefined);
});

test("the detail wins over the catalog, and carries the holding", () => {
  const product = selectProduct("arb", reads({ detail: { data: allocation({ caller_access: "invest" }), error: null, isLoading: false }, catalog: [allocation({ caller_access: "view" })], positions: { data: [position], isLoading: false } }));
  assert.ok(product);
  assert.equal(product.allocation?.caller_access, "invest");
  assert.equal(product.position, position);
});

test("a closed product the detail still returns is closed, not locked", () => {
  const product = selectProduct("arb", reads({ detail: { data: allocation({ state: "closed", caller_access: "view" }), error: null, isLoading: false } }));
  assert.ok(product);
  assert.equal(isClosed(product), true);
  assert.equal(isLocked(product), false);
  assert.equal(blockedReasonKey(product, null), "invest.blocked.closed");
});

test("any other failure falls back to what the catalog and the positions know", () => {
  const failed = { data: undefined, error: Object.assign(new Error("boom"), { status: 502 }), isLoading: false };
  const listed = selectProduct("arb", reads({ detail: failed, catalog: [allocation()] }));
  assert.equal(listed?.allocation?.service, "arb");
  const held = selectProduct("arb", reads({ detail: failed, positions: { data: [position], isLoading: false } }));
  assert.equal(held?.allocation, null);
  assert.equal(selectProduct("arb", reads({ detail: failed })), null);
});
