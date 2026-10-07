// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// The dashboard's chart engine is downloaded on demand, so the plot mounts before it can
// draw. These pin the three ways that download can end for a mounted plot — it lands, the
// plot is gone first, it fails — with a fake engine (`shared/__tests__`) the test releases
// or fails by hand.
import assert from "node:assert/strict";
import test from "node:test";

import { installModuleHooks, renderHook, settle } from "../../../shared/__tests__/react-hook-harness.ts";
import { fakeChartEngine } from "../../../shared/__tests__/fake-chart-engine.ts";
import { chunkLoadError } from "../../../shared/__tests__/fake-download.ts";
import type { NavSeries } from "../../../entities/fund/lib/nav-series.ts";
import type { PerfFormat } from "./use-perf-chart.ts";

type Hook = typeof import("./use-perf-chart.ts");

installModuleHooks();

let cases = 0;
/** A fresh copy of the hook's module — its once-per-page engine cache starts empty — and
 *  the fake engine that copy downloads. */
const freshHook = async () => {
  cases += 1;
  const engine = fakeChartEngine(String(cases));
  const mod: Hook = await import(`./use-perf-chart.ts?case=${cases}`);
  return { engine, mod };
};

const SERIES: NavSeries = {
  performance: [
    { time: 1_700_000_000, value: 0 },
    { time: 1_700_086_400, value: 1.5 },
  ],
  participation: [
    { time: 1_700_000_000, value: 100 },
    { time: 1_700_086_400, value: 101.5 },
  ],
  baseNav: 1,
};
const FORMAT: PerfFormat = { performance: (pct) => `${pct}%`, participation: (usdt) => `${usdt} USDT` };

/** Mounts the hook the way `PerfPlot` does: one host for the plot's life. */
const mountPlot = (usePerfChart: Hook["usePerfChart"]) => {
  const host = { current: { clientWidth: 640, clientHeight: 256 } as unknown as HTMLDivElement };
  return renderHook(() => usePerfChart(host, SERIES, FORMAT), undefined);
};

/** Collects the process's unhandled rejections while `body` runs. */
async function unhandledDuring(body: () => Promise<void>): Promise<unknown[]> {
  const seen: unknown[] = [];
  const record = (reason: unknown) => void seen.push(reason);
  process.on("unhandledRejection", record);
  try {
    await body();
  } finally {
    process.off("unhandledRejection", record);
  }
  return seen;
}

test("both lines are drawn once the engine lands", async () => {
  const { engine, mod } = await freshHook();
  const plot = await mountPlot(mod.usePerfChart);

  await settle(() => engine.arrive());

  assert.deepEqual(
    engine.charts[0]?.series.map((s) => s.data.at(-1)?.length),
    [2, 2],
  );
  await plot.unmount();
});

test("a plot unmounted before the engine lands creates no chart and leaves no observer", async () => {
  const { engine, mod } = await freshHook();
  const plot = await mountPlot(mod.usePerfChart);

  await plot.unmount();
  await settle(() => engine.arrive());

  assert.equal(engine.charts.length, 0);
  assert.equal(engine.observing, 0);
});

test("an unmounted plot removes its chart and its resize observer", async () => {
  const { engine, mod } = await freshHook();
  const plot = await mountPlot(mod.usePerfChart);
  await settle(() => engine.arrive());

  await plot.unmount();

  assert.equal(engine.charts[0]?.removed, true);
  assert.equal(engine.observing, 0);
});

test("the preload and the plot share one download", async () => {
  const { engine, mod } = await freshHook();
  mod.preloadPerfEngine();
  const plot = await mountPlot(mod.usePerfChart);

  await settle(() => engine.arrive());

  assert.equal(engine.downloads, 1);
  assert.equal(engine.charts.length, 1);
  await plot.unmount();
});

test("an engine that fails to download leaves no unhandled rejection behind", async () => {
  const { engine, mod } = await freshHook();
  const plot = await mountPlot(mod.usePerfChart);

  const unhandled = await unhandledDuring(() => settle(() => engine.fail(new Error("chunk refused"))));

  assert.deepEqual(unhandled, []);
  await plot.unmount();
});

test("after a failed download the hook asks for the engine again on the next plot", async () => {
  const { engine, mod } = await freshHook();
  mod.preloadPerfEngine();
  await settle(() => engine.fail(new Error("chunk refused")));

  const plot = await mountPlot(mod.usePerfChart);
  await settle(() => engine.arrive());

  assert.equal(engine.downloads, 2);
  assert.equal(engine.charts.length, 1);
  await plot.unmount();
});

test("the plot reads loading until the engine lands, then ready", async () => {
  const { engine, mod } = await freshHook();
  const plot = await mountPlot(mod.usePerfChart);
  assert.deepEqual(plot.current, { kind: "loading" });

  await settle(() => engine.arrive());

  assert.deepEqual(plot.current, { kind: "ready" });
  await plot.unmount();
});

test("a download that fails reads failed, carrying the bundler's error", async () => {
  const { engine, mod } = await freshHook();
  const plot = await mountPlot(mod.usePerfChart);
  const refused = chunkLoadError();

  await settle(() => engine.fail(refused));

  assert.deepEqual(plot.current, { kind: "failed", error: refused });
  assert.equal(engine.charts.length, 0);
  await plot.unmount();
});

test("the surface's own reader starts the download before any plot exists", async () => {
  const { engine, mod } = await freshHook();

  const surface = await renderHook(() => mod.usePerfEngine(), undefined);
  const beforeArrival = surface.current;
  // `arrive` waits for a download to start: without one this would never return.
  await settle(() => engine.arrive());

  assert.deepEqual(beforeArrival, { kind: "loading" });
  assert.deepEqual(surface.current, { kind: "ready" });
  await surface.unmount();
});

test("after a failure the next reader is back to loading, then ready on the second download", async () => {
  const { engine, mod } = await freshHook();
  const first = await renderHook(() => mod.usePerfEngine(), undefined);
  await settle(() => engine.fail(chunkLoadError()));
  await first.unmount();

  const second = await renderHook(() => mod.usePerfEngine(), undefined);
  const onMount = second.current;
  await settle(() => engine.arrive());

  assert.deepEqual(onMount, { kind: "loading" });
  assert.deepEqual(second.current, { kind: "ready" });
  assert.equal(engine.downloads, 2);
  await second.unmount();
});
