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
