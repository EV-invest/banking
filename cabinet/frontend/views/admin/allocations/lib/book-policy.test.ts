// Run with `npm run test` (Node's built-in runner, native type-stripping).
import assert from "node:assert/strict";
import test from "node:test";

import { bookPolicyDraft, bookPolicyProblem, hasAdvancedTerms, needsAcknowledgement, setBookPolicyBody } from "./book-policy.ts";

test("the form is seeded from the policy as it stands, rates as percents", () => {
  assert.deepEqual(bookPolicyDraft({ service: "arb", book_open: true, taker_fee_bps: 25, price_tick: "0.01", lot_size: "1", market_slippage_bps: 100 }), {
    open: true,
    takerFeePct: "0.25",
    tick: "0.01",
    lot: "1",
    slippagePct: "1",
    allowUnbackedTrading: false,
  });
  // No policy row yet: closed, no fee, nothing to keep, nothing acknowledged.
  assert.deepEqual(bookPolicyDraft(null), { open: false, takerFeePct: "0", tick: "", lot: "", slippagePct: "", allowUnbackedTrading: false });
  // The hub's zero spellings for an unset term seed an empty field, which the body then
  // leaves out — so saving an untouched form changes nothing.
  const zeros = bookPolicyDraft({ service: "arb", book_open: false, taker_fee_bps: 0, price_tick: "0", lot_size: "0", market_slippage_bps: 0, updated_at: "0" });
  assert.deepEqual(zeros, { open: false, takerFeePct: "0", tick: "", lot: "", slippagePct: "", allowUnbackedTrading: false });
  assert.deepEqual(setBookPolicyBody("arb", zeros), { service: "arb", book_open: false, taker_fee_bps: 0, allow_unbacked_trading: false });
});

test("the acknowledgement round-trips: absent on the wire reads as not given", () => {
  assert.equal(bookPolicyDraft({ service: "arb", book_open: true, taker_fee_bps: 0 }).allowUnbackedTrading, false);
  assert.equal(bookPolicyDraft({ service: "arb", book_open: true, taker_fee_bps: 0, allow_unbacked_trading: true }).allowUnbackedTrading, true);
  // Sent even when false — the hub's default, spelled out rather than left to the BFF.
  const body = setBookPolicyBody("arb", { open: true, takerFeePct: "0", tick: "", lot: "", slippagePct: "", allowUnbackedTrading: true });
  assert.equal(body?.allow_unbacked_trading, true);
});

test("rates go to the wire as basis points; untouched advanced fields stay absent", () => {
  const body = setBookPolicyBody("arb", { open: true, takerFeePct: "0.25", tick: "", lot: "", slippagePct: "", allowUnbackedTrading: false });
  assert.deepEqual(body, { service: "arb", book_open: true, taker_fee_bps: 25, allow_unbacked_trading: false });
  assert.equal(body && "price_tick" in body, false);
  const full = setBookPolicyBody("arb", { open: false, takerFeePct: "2", tick: "0.001", lot: "0.5", slippagePct: "0.5", allowUnbackedTrading: true });
  assert.deepEqual(full, { service: "arb", book_open: false, taker_fee_bps: 200, allow_unbacked_trading: true, price_tick: "0.001", lot_size: "0.5", market_slippage_bps: 50 });
});

test("a draft is refused for the field that is wrong", () => {
  const ok = { open: true, takerFeePct: "0.25", tick: "", lot: "", slippagePct: "", allowUnbackedTrading: false };
  assert.equal(bookPolicyProblem(ok), null);
  assert.equal(bookPolicyProblem({ ...ok, takerFeePct: "" }), "fee");
  assert.equal(bookPolicyProblem({ ...ok, takerFeePct: "0.005" }), "fee");
  assert.equal(bookPolicyProblem({ ...ok, takerFeePct: "150" }), "fee");
  assert.equal(bookPolicyProblem({ ...ok, tick: "abc" }), "tick");
  assert.equal(bookPolicyProblem({ ...ok, lot: "-1" }), "lot");
  assert.equal(bookPolicyProblem({ ...ok, slippagePct: "x" }), "slippage");
  assert.equal(setBookPolicyBody("arb", { ...ok, lot: "-1" }), null);
});

test("the hub's zero spellings for an unset term do not count as a term", () => {
  assert.equal(hasAdvancedTerms(null), false);
  assert.equal(hasAdvancedTerms({ service: "arb", book_open: false, taker_fee_bps: 0, price_tick: "0", lot_size: "0", market_slippage_bps: 0 }), false);
  assert.equal(hasAdvancedTerms({ service: "arb", book_open: true, taker_fee_bps: 25, price_tick: "0.01" }), true);
  assert.equal(hasAdvancedTerms({ service: "arb", book_open: true, taker_fee_bps: 25, market_slippage_bps: 50 }), true);
});

test("an open book on a product held in kind needs the acknowledgement; nothing else does", () => {
  const open = { open: true, allowUnbackedTrading: false };
  assert.equal(needsAcknowledgement(open, { backing: "in_kind" }), true);
  // Ticked: the hub takes it.
  assert.equal(needsAcknowledgement({ ...open, allowUnbackedTrading: true }, { backing: "in_kind" }), false);
  // A closed book asks nothing, whatever stands behind the units.
  assert.equal(needsAcknowledgement({ ...open, open: false }, { backing: "in_kind" }), false);
  // Cash-backed — spelled out or absent, the hub's default — never asks.
  assert.equal(needsAcknowledgement(open, { backing: "cash" }), false);
  assert.equal(needsAcknowledgement(open, {}), false);
});
