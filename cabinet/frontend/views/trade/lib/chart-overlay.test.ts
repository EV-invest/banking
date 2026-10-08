// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// What the price chart draws over its canvas. A chunk that never arrived gets "Reload page"
// (the bundler will not fetch it again without one); every other failure keeps the error
// text the pane always showed.
import assert from "node:assert/strict";
import test from "node:test";

import { chartOverlay } from "./chart-overlay.ts";

const chunkLoadError = () => Object.assign(new Error("Failed to load chunk /cabinet/_next/static/chunks/a.js"), { name: "ChunkLoadError" });

test("a drawn chart has nothing over it", () => {
  assert.deepEqual(chartOverlay({ kind: "ready" }), { kind: "none" });
});

test("a loading chart and an empty market keep their own overlays", () => {
  assert.deepEqual(chartOverlay({ kind: "loading" }), { kind: "loading" });
  assert.deepEqual(chartOverlay({ kind: "empty" }), { kind: "empty" });
});

test("an engine chunk that never arrived offers a reload", () => {
  assert.deepEqual(chartOverlay({ kind: "failed", error: chunkLoadError() }), { kind: "reload" });
});

test("any other failure keeps the error text, carrying the error", () => {
  const refused = Object.assign(new Error("candles unavailable"), { name: "RequestError" });

  assert.deepEqual(chartOverlay({ kind: "failed", error: refused }), { kind: "error", error: refused });
});
