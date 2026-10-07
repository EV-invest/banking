// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// Which state the dashboard's plot is in. The engine is downloaded on demand, so the plot
// keeps ONE skeleton until both the history and the engine are here — never an empty
// bordered frame between the two — and a chunk that never arrived offers a reload.
import assert from "node:assert/strict";
import test from "node:test";

import { perfView, type PerfViewInput } from "./perf-view.ts";

const chunkLoadError = () => Object.assign(new Error("Failed to load chunk /cabinet/_next/static/chunks/a.js"), { name: "ChunkLoadError" });
const loaded: PerfViewInput = { allocation: "service_arb", historyLoading: false, historyFailed: false, drawable: true, engine: { kind: "ready" } };

test("a history with marks and an engine that is here draws the plot", () => {
  assert.deepEqual(perfView(loaded), { kind: "plot" });
});

test("history in hand but the engine still downloading keeps the skeleton", () => {
  assert.deepEqual(perfView({ ...loaded, engine: { kind: "loading" } }), { kind: "skeleton" });
});

test("the engine in hand but the history still loading keeps the skeleton", () => {
  assert.deepEqual(perfView({ ...loaded, historyLoading: true }), { kind: "skeleton" });
});

test("an allocation still being resolved keeps the skeleton", () => {
  assert.deepEqual(perfView({ ...loaded, allocation: null }), { kind: "skeleton" });
});

test("nothing to plot needs no engine: an empty history is empty while the engine downloads", () => {
  assert.deepEqual(perfView({ ...loaded, drawable: false, engine: { kind: "loading" } }), { kind: "empty" });
});

test("an empty history stays empty even when the engine failed", () => {
  assert.deepEqual(perfView({ ...loaded, drawable: false, engine: { kind: "failed", error: chunkLoadError() } }), { kind: "empty" });
});

test("a failed history read is reported before the engine's verdict", () => {
  assert.deepEqual(perfView({ ...loaded, historyFailed: true, drawable: false, engine: { kind: "failed", error: chunkLoadError() } }), { kind: "history-error" });
});

test("an engine chunk that never arrived offers a reload", () => {
  assert.deepEqual(perfView({ ...loaded, engine: { kind: "failed", error: chunkLoadError() } }), { kind: "reload" });
});

test("any other engine failure is shown as an error, carrying it", () => {
  const broken = new TypeError("createChart is not a function");

  assert.deepEqual(perfView({ ...loaded, engine: { kind: "failed", error: broken } }), { kind: "engine-error", error: broken });
});
