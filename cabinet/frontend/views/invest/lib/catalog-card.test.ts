// Run with `npm run test` (Node's built-in runner, native type-stripping).
import assert from "node:assert/strict";
import test from "node:test";

import type { Allocation, FundNav, Position } from "../../../shared/contracts/index.ts";

import { cardCta, liquidity } from "./catalog-card.ts";
import type { Product } from "./product.ts";

const allocation = (over: Partial<Allocation> = {}): Allocation => ({
  service: "arb",
  title: "Arbitrage",
  summary: "",
  state: "open",
  created_at: "0",
  updated_at: "0",
  unit_cap: "1000",
  access: "view",
  caller_access: "invest",
  ...over,
});

const product = (over: Partial<Product> = {}, alloc: Partial<Allocation> = {}): Product => ({
  service: "arb",
  title: "Arbitrage",
  summary: "",
  icon: undefined,
  allocation: allocation(alloc),
  position: null,
  ...over,
});

const nav = (remaining: string): FundNav => ({ service: "arb", nav: "1", units_outstanding: "10", unit_cap: "1000", remaining_capacity: remaining });
const held: Position = { service: "arb", units: "10", nav: "1", value: "10", cost_basis: "10", pnl: "0" };

test("an open product open to a verified caller is simply investable", () => {
  assert.equal(cardCta({ product: product(), nav: nav("990"), bookOpen: false, gated: false }), "invest");
});

test("a holding outranks every gate — the page is where the holder acts", () => {
  assert.equal(cardCta({ product: product({ position: held }, { caller_access: "view" }), nav: nav("0"), bookOpen: true, gated: true }), "manage");
});

test("a zero-unit position is not a holding", () => {
  assert.equal(cardCta({ product: product({ position: { ...held, units: "0" } }), nav: null, bookOpen: false, gated: false }), "invest");
});

test("an operator's lock is named ahead of the caller's own tier", () => {
  assert.equal(cardCta({ product: product({}, { caller_access: "view" }), nav: nav("990"), bookOpen: false, gated: true }), "locked");
});

test("tier 0 is sent to verify rather than to a subscription it cannot fund", () => {
  assert.equal(cardCta({ product: product(), nav: nav("990"), bookOpen: false, gated: true }), "verify");
});

test("a fully issued product with a book sends the caller to the book", () => {
  assert.equal(cardCta({ product: product(), nav: nav("0"), bookOpen: true, gated: false }), "trade");
});

test("a fully issued product with no book has nothing to offer but the page", () => {
  assert.equal(cardCta({ product: product(), nav: nav("0"), bookOpen: false, gated: false }), "view");
});

test("a closed product nobody holds is only viewable", () => {
  assert.equal(cardCta({ product: product({ allocation: null }), nav: null, bookOpen: true, gated: false }), "view");
  assert.equal(cardCta({ product: product({}, { state: "closed" }), nav: null, bookOpen: true, gated: false }), "view");
});

test("an unread NAV never blocks: capacity is unknown, not zero", () => {
  assert.equal(cardCta({ product: product(), nav: null, bookOpen: true, gated: false }), "invest");
});

test("liquidity: in-kind exits through the book whatever the book is doing", () => {
  assert.equal(liquidity(product({}, { backing: "in_kind" }), false), "book");
  assert.equal(liquidity(product({}, { backing: "in_kind" }), true), "book");
});

test("liquidity: cash-backed redeems at NAV, with the book as a second door when open", () => {
  assert.equal(liquidity(product(), false), "navQueued");
  assert.equal(liquidity(product(), true), "navOrBook");
  assert.equal(liquidity(product({ allocation: null }), true), "navOrBook");
});
