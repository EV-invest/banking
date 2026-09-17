// Run with `npm run test` (Node's built-in runner, native type-stripping).
import assert from "node:assert/strict";
import test from "node:test";

import type { FundNav } from "../../../shared/contracts/index.ts";

import { canSubmit, checkSubscribe } from "./subscribe-check.ts";

const nav = (over: Partial<FundNav> = {}): FundNav => ({ nav: "2", remaining_capacity: "1000", ...over });

test("an empty amount previews nothing and is not an issue", () => {
  const c = checkSubscribe({ amount: "", available: "100", nav: nav() });
  assert.equal(c.preview, null);
  assert.equal(c.issue, null);
  assert.equal(canSubmit(c), false);
});

test("an affordable amount under the cap goes through", () => {
  const c = checkSubscribe({ amount: "10", available: "100", nav: nav() });
  assert.equal(c.preview, 5n * 10n ** 18n);
  assert.equal(c.issue, null);
  assert.equal(canSubmit(c), true);
});

test("more than the available balance is insufficient, even by one base unit", () => {
  assert.equal(checkSubscribe({ amount: "100", available: "100", nav: nav() }).issue, null);
  assert.equal(checkSubscribe({ amount: "100.000000000000000001", available: "100", nav: nav() }).issue, "insufficient");
  assert.equal(checkSubscribe({ amount: "1", available: "0", nav: nav() }).issue, "insufficient");
});

test("an unread balance gates nothing", () => {
  // The hub refuses the submit itself; a failed wallet read must not read as an empty wallet.
  const c = checkSubscribe({ amount: "1000000", available: null, nav: nav({ remaining_capacity: "1000000000" }) });
  assert.equal(c.issue, null);
  assert.equal(canSubmit(c), true);
});

test("insufficient wins over the cap and over dust — the top-up is the fix", () => {
  assert.equal(checkSubscribe({ amount: "5000", available: "100", nav: nav({ remaining_capacity: "10" }) }).issue, "insufficient");
  assert.equal(checkSubscribe({ amount: "0.5", available: "0.1", nav: nav() }).issue, "insufficient");
});

test("over the cap when the units bought exceed the headroom", () => {
  const c = checkSubscribe({ amount: "30", available: "100", nav: nav({ remaining_capacity: "10" }) });
  assert.equal(c.issue, "overCap");
  assert.equal(c.headroom, 10n * 10n ** 18n);
  assert.equal(canSubmit(c), false);
});

test("dust: an amount that floors to zero units", () => {
  const c = checkSubscribe({ amount: "0.000000000000000001", available: "100", nav: nav() });
  assert.equal(c.preview, 0n);
  assert.equal(c.issue, "dust");
  assert.equal(canSubmit(c), false);
});

test("no mark yet: no preview, no verdict, no submit", () => {
  const c = checkSubscribe({ amount: "10", available: "100", nav: null });
  assert.equal(c.preview, null);
  assert.equal(c.headroom, null);
  assert.equal(c.issue, null);
  assert.equal(canSubmit(c), false);
});
