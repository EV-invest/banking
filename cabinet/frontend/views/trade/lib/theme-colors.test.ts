// Run with `npm run test` (Node's built-in runner, native type-stripping).
import assert from "node:assert/strict";
import test from "node:test";

import { readChartPalette } from "./theme-colors.ts";

test("without a document the whole palette is the fallback", () => {
  assert.equal(typeof document, "undefined");
  assert.deepEqual(readChartPalette(), {
    background: "transparent",
    text: "#9a9a9a",
    grid: "#2a2a2a",
    up: "#2e9e5b",
    down: "#ef5b52",
  });
});
