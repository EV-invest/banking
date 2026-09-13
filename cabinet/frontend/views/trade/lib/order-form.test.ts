// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// The rules the hub enforces with a 400, stated before the submit: tick, lot, funds — and
// the body a market order becomes, which carries neither price nor tif.
import assert from "node:assert/strict";
import test from "node:test";

import { EMPTY_ORDER_DRAFT, asDecimal, orderDraftProblem, orderNotional, orderSubmissionFor, placeOrderBody, takerFee, type OrderContext, type OrderDraft } from "./order-form.ts";

const context: OrderContext = {
  policy: { taker_fee_bps: 25, price_tick: "0.01", lot_size: "1" },
  availableCash: "1000",
  availableUnits: "50",
  referencePrice: "1.02",
};

const limitBuy: OrderDraft = { ...EMPTY_ORDER_DRAFT, price: "1.05", size: "100" };

test("notional and fee are exact, and post-only pays no taker fee", () => {
  const notional = orderNotional(limitBuy, context);
  assert.equal(notional && asDecimal(notional), "105");
  assert.equal(asDecimal(takerFee(notional ?? 0n, limitBuy, context.policy)), "0.2625");
  assert.equal(takerFee(notional ?? 0n, { ...limitBuy, tif: "alo" }, context.policy), 0n);
  // A market order is valued at the reference price.
  const market = orderNotional({ ...limitBuy, kind: "market" }, context);
  assert.equal(market && asDecimal(market), "102");
  assert.equal(orderNotional({ ...limitBuy, size: "" }, context), null);
});

test("a draft is refused in the order the trader would notice", () => {
  assert.equal(orderDraftProblem({ ...limitBuy, price: "" }, context), "price");
  assert.equal(orderDraftProblem({ ...limitBuy, price: "1.005" }, context), "tick");
  assert.equal(orderDraftProblem({ ...limitBuy, size: "0" }, context), "size");
  assert.equal(orderDraftProblem({ ...limitBuy, size: "1.5" }, context), "lot");
  assert.equal(orderDraftProblem({ ...limitBuy, size: "9" }, context), null);
  // A tick or lot of zero (the hub's "unset") admits any figure.
  assert.equal(orderDraftProblem({ ...limitBuy, price: "1.005", size: "1.5" }, { ...context, policy: { taker_fee_bps: 0, price_tick: "0", lot_size: "0" } }), null);
});

test("a buy needs notional plus fee in cash; a sell needs the units", () => {
  // 1000 × 1.05 = 1050 > 1000 available.
  assert.equal(orderDraftProblem({ ...limitBuy, size: "1000" }, context), "funds");
  // 952 × 1.05 = 999.6, + 0.25% fee = 2.499 → 1002.099 > 1000: the fee is what tips it.
  assert.equal(orderDraftProblem({ ...limitBuy, size: "952" }, context), "funds");
  assert.equal(orderDraftProblem({ ...limitBuy, size: "950" }, context), null);
  assert.equal(orderDraftProblem({ ...limitBuy, side: "sell", size: "51" }, context), "funds");
  assert.equal(orderDraftProblem({ ...limitBuy, side: "sell", size: "50" }, context), null);
  // Unknown balances do not block — the hub is the authority and answers 400 if short.
  assert.equal(orderDraftProblem({ ...limitBuy, size: "1000" }, { ...context, availableCash: null }), null);
});

test("a market order needs a quote to trade against", () => {
  assert.equal(orderDraftProblem({ ...limitBuy, kind: "market", price: "" }, context), null);
  assert.equal(orderDraftProblem({ ...limitBuy, kind: "market" }, { ...context, referencePrice: null }), "noQuote");
});

test("the body carries price and tif for a limit and neither for a market order", () => {
  assert.deepEqual(placeOrderBody("arb", { ...limitBuy, tif: "alo", price: " 1.05 " }, context, "cid-1"), {
    service: "arb",
    side: "buy",
    kind: "limit",
    tif: "alo",
    price: "1.05",
    size: "100",
    client_order_id: "cid-1",
  });
  const market = placeOrderBody("arb", { ...limitBuy, kind: "market", side: "sell", size: "10" }, context, "cid-2");
  assert.deepEqual(market, { service: "arb", side: "sell", kind: "market", size: "10", client_order_id: "cid-2" });
  assert.equal(market && "price" in market, false);
  assert.equal(placeOrderBody("arb", { ...limitBuy, size: "" }, context, "cid-3"), null);
});

test("a retry reuses its client_order_id; an edited intent mints a new one", () => {
  let n = 0;
  const mint = () => `cid-${++n}`;
  const first = orderSubmissionFor(null, "arb", limitBuy, mint);
  assert.equal(first.id, "cid-1");
  assert.equal(orderSubmissionFor(first, "arb", limitBuy, mint), first);
  assert.equal(orderSubmissionFor(first, "arb", { ...limitBuy, size: " 100 " }, mint), first);
  // The price typed for a limit is not part of a market intent, so switching kinds and
  // back with a different stale price is still the same market order.
  const market = orderSubmissionFor(first, "arb", { ...limitBuy, kind: "market" }, mint);
  assert.equal(market.id, "cid-2");
  assert.equal(orderSubmissionFor(market, "arb", { ...limitBuy, kind: "market", price: "9" }, mint), market);
  assert.equal(orderSubmissionFor(first, "arb", { ...limitBuy, side: "sell" }, mint).id, "cid-3");
  assert.equal(orderSubmissionFor(first, "other", limitBuy, mint).id, "cid-4");
});
