// Run with `npm run test` (Node's built-in runner, native type-stripping).
import assert from "node:assert/strict";
import test from "node:test";

import { bookPath } from "./wire.ts";

test("parameters the caller has are sent, the ones it does not are absent", () => {
  assert.equal(bookPath("/api/book/trades", { service: "arb", limit: 40 }), "/api/book/trades?service=arb&limit=40");
  // `undefined` and `""` both mean "the BFF's default" — neither may reach the wire as text.
  assert.equal(bookPath("/api/book/orders", { service: undefined }), "/api/book/orders");
  assert.equal(bookPath("/api/book/orders", { service: "" }), "/api/book/orders");
  assert.equal(bookPath("/api/book/candles", { service: "arb", resolution: "1h", from: 1_700_000_000, to: undefined }), "/api/book/candles?service=arb&resolution=1h&from=1700000000");
});

test("a service id that carries query metacharacters arrives intact", () => {
  assert.equal(bookPath("/api/book", { service: "a&b/c d" }), "/api/book?service=a%26b%2Fc%20d");
});
