// Run with `npm run test` (Node's built-in runner, native type-stripping).
//
// The terminal's chart engine is downloaded on demand while the candle history is fetched
// in parallel, so the pane's state now depends on which of the two lands first. These pin
// what the pane shows for each order of arrival, and that a pane gone before the engine
// arrived leaves nothing behind. The engine is a fake (`shared/__tests__`) whose download
// the test releases or fails by hand; the feed is a fake whose history the test answers.
import assert from "node:assert/strict";
import test from "node:test";

import { deferred, installModuleHooks, renderHook, settle, type Deferred } from "../../../shared/__tests__/react-hook-harness.ts";
import { fakeChartEngine } from "../../../shared/__tests__/fake-chart-engine.ts";
import { chunkLoadError } from "../../../shared/__tests__/fake-download.ts";
import type { CandleResolution } from "../../../shared/contracts/book.ts";
import type { Bar } from "./candles.ts";
import type { ChartFeed } from "./chart-feed.ts";
import { chartOverlay } from "./chart-overlay.ts";

type Hook = typeof import("./use-candle-chart.ts");

installModuleHooks({ "@/views/trade/lib/chart-feed": new URL("./__tests__/chart-feed.stub.ts", import.meta.url).href });

let cases = 0;
/** A fresh copy of the hook's module — its once-per-page engine cache starts empty — and
 *  the fake engine that copy downloads. */
const freshHook = async () => {
  cases += 1;
  const engine = fakeChartEngine(String(cases));
  const mod: Hook = await import(`./use-candle-chart.ts?case=${cases}`);
  return { engine, useCandleChart: mod.useCandleChart };
};

function fakeFeed() {
  const requests: { resolution: CandleResolution; reply: Deferred<Bar[]> }[] = [];
  const subscriptions: { resolution: CandleResolution; push: (bar: Bar) => void; open: boolean }[] = [];
  const feed: ChartFeed = {
    history(resolution) {
      const reply = deferred<Bar[]>();
      requests.push({ resolution, reply });
      return reply.promise;
    },
    subscribe(resolution, _last, push) {
      const subscription = { resolution, push, open: true };
      subscriptions.push(subscription);
      return () => {
        subscription.open = false;
      };
    },
  };
  return { feed, requests, subscriptions };
}

const bar = (time: number, close: string): Bar => ({ time, open: close, high: close, low: close, close, volume: "1" });
/** The pane's host element, one per mount — the hook keys the engine's lifetime on it. */
const host = () => ({ current: { clientWidth: 640, clientHeight: 320 } as unknown as HTMLDivElement });
/** Mounts the hook the way `ChartPane` does: one host and one feed for the pane's life. */
const mountPane = (useCandleChart: Hook["useCandleChart"], feed: ChartFeed, resolution: CandleResolution) => {
  const el = host();
  return renderHook((r: CandleResolution) => useCandleChart(el, feed, r), resolution);
};

test("a pane mounted before the engine arrives reads loading, even with its history in hand", async () => {
  const { engine, useCandleChart } = await freshHook();
  const { feed, requests } = fakeFeed();
  const chart = await mountPane(useCandleChart, feed, "1h");

  await settle(() => requests[0]?.reply.resolve([bar(3600, "1.00")]));

  assert.deepEqual(chart.current, { kind: "loading" });
  assert.equal(engine.charts.length, 0);
  await chart.unmount();
});

test("the history request leaves on mount, before the engine has arrived", async () => {
  const { useCandleChart } = await freshHook();
  const { feed, requests } = fakeFeed();
  const chart = await mountPane(useCandleChart, feed, "1h");

  assert.deepEqual(
    requests.map((r) => r.resolution),
    ["1h"],
  );
  await chart.unmount();
});

test("bars that came first are drawn the moment the engine lands, and the pane turns ready", async () => {
  const { engine, useCandleChart } = await freshHook();
  const { feed, requests, subscriptions } = fakeFeed();
  const chart = await mountPane(useCandleChart, feed, "1h");
  await settle(() => requests[0]?.reply.resolve([bar(3600, "1.00"), bar(7200, "1.25")]));

  await settle(() => engine.arrive());

  assert.deepEqual(chart.current, { kind: "ready" });
  assert.deepEqual(
    engine.charts[0]?.series[0]?.data.map((points) => points.length),
    [2],
  );
  assert.equal(subscriptions.filter((s) => s.open).length, 1);
  await chart.unmount();
});

test("a pane unmounted before the engine arrives creates no chart and leaves no subscription", async () => {
  const { engine, useCandleChart } = await freshHook();
  const { feed, requests, subscriptions } = fakeFeed();
  const chart = await mountPane(useCandleChart, feed, "1h");

  await chart.unmount();
  await settle(() => {
    requests[0]?.reply.resolve([bar(3600, "1.00")]);
    return engine.arrive();
  });

  assert.equal(engine.charts.length, 0);
  assert.equal(engine.observing, 0);
  assert.equal(subscriptions.length, 0);
});

test("an unmounted pane removes its chart, its resize observer and its live subscription", async () => {
  const { engine, useCandleChart } = await freshHook();
  const { feed, requests, subscriptions } = fakeFeed();
  const chart = await mountPane(useCandleChart, feed, "1h");
  await settle(() => {
    requests[0]?.reply.resolve([bar(3600, "1.00")]);
    return engine.arrive();
  });

  await chart.unmount();

  assert.equal(engine.charts[0]?.removed, true);
  assert.equal(engine.observing, 0);
  assert.equal(subscriptions.filter((s) => s.open).length, 0);
});

test("an engine that fails to download turns the pane failed, not an endless loading", async () => {
  const { engine, useCandleChart } = await freshHook();
  const { feed, requests } = fakeFeed();
  const chart = await mountPane(useCandleChart, feed, "1h");
  const refused = new Error("chunk refused");

  await settle(() => {
    requests[0]?.reply.resolve([bar(3600, "1.00")]);
    return engine.fail(refused);
  });

  assert.deepEqual(chart.current, { kind: "failed", error: refused });
  await chart.unmount();
});

test("after a failed download the hook asks for the engine again on the next mount", async () => {
  const { engine, useCandleChart } = await freshHook();
  const { feed, requests } = fakeFeed();
  const first = await mountPane(useCandleChart, feed, "1h");
  await settle(() => engine.fail(new Error("chunk refused")));
  await first.unmount();

  const second = await mountPane(useCandleChart, feed, "1h");
  await settle(() => {
    requests[1]?.reply.resolve([bar(3600, "1.00")]);
    return engine.arrive();
  });

  assert.equal(engine.downloads, 2);
  assert.deepEqual(second.current, { kind: "ready" });
  await second.unmount();
});

test("a history that fails before the engine arrives turns the pane failed", async () => {
  const { useCandleChart } = await freshHook();
  const { feed, requests } = fakeFeed();
  const chart = await mountPane(useCandleChart, feed, "1h");
  const refused = new Error("candles unavailable");

  await settle(() => requests[0]?.reply.reject(refused));

  assert.deepEqual(chart.current, { kind: "failed", error: refused });
  await chart.unmount();
});

test("switching resolution reads loading from its first render, not the previous resolution's chart", async () => {
  const { engine, useCandleChart } = await freshHook();
  const { feed, requests } = fakeFeed();
  const chart = await mountPane(useCandleChart, feed, "1h");
  await settle(() => {
    requests[0]?.reply.resolve([bar(3600, "1.00")]);
    return engine.arrive();
  });
  const rendersBefore = chart.history.length;

  await chart.rerender("1d");

  assert.deepEqual(chart.history[rendersBefore], { kind: "loading" });
  assert.deepEqual(chart.current, { kind: "loading" });
  assert.deepEqual(
    requests.map((r) => r.resolution),
    ["1h", "1d"],
  );
  await chart.unmount();
});

test("a switch made while the engine is still loading never draws the old resolution's bars", async () => {
  const { engine, useCandleChart } = await freshHook();
  const { feed, requests, subscriptions } = fakeFeed();
  const chart = await mountPane(useCandleChart, feed, "1h");
  await settle(() => requests[0]?.reply.resolve([bar(3600, "1.00")]));
  await chart.rerender("1d");

  await settle(() => engine.arrive());

  assert.deepEqual(chart.current, { kind: "loading" });
  assert.deepEqual(engine.charts[0]?.series[0]?.data, []);
  assert.equal(subscriptions.length, 0);
  await chart.unmount();
});

test("a late answer for the previous resolution does not overwrite the current one", async () => {
  const { engine, useCandleChart } = await freshHook();
  const { feed, requests } = fakeFeed();
  const chart = await mountPane(useCandleChart, feed, "1h");
  await settle(() => engine.arrive());
  await chart.rerender("1d");
  await settle(() => requests[1]?.reply.resolve([]));

  await settle(() => requests[0]?.reply.resolve([bar(3600, "1.00")]));

  assert.deepEqual(chart.current, { kind: "empty" });
  assert.deepEqual(
    engine.charts[0]?.series[0]?.data.map((points) => points.length),
    [0],
  );
  await chart.unmount();
});

test("an empty market reads empty until the first live bar, then ready", async () => {
  const { engine, useCandleChart } = await freshHook();
  const { feed, requests, subscriptions } = fakeFeed();
  const chart = await mountPane(useCandleChart, feed, "1h");
  await settle(() => {
    requests[0]?.reply.resolve([]);
    return engine.arrive();
  });
  assert.deepEqual(chart.current, { kind: "empty" });

  await settle(() => subscriptions[0]?.push(bar(3600, "1.00")));

  assert.deepEqual(chart.current, { kind: "ready" });
  assert.equal(engine.charts[0]?.series[0]?.updates.length, 1);
  await chart.unmount();
});

test("an engine chunk that never arrives ends in the pane's reload overlay", async () => {
  const { engine, useCandleChart } = await freshHook();
  const { feed, requests } = fakeFeed();
  const chart = await mountPane(useCandleChart, feed, "1h");

  await settle(() => {
    requests[0]?.reply.resolve([bar(3600, "1.00")]);
    return engine.fail(chunkLoadError());
  });

  assert.deepEqual(chartOverlay(chart.current), { kind: "reload" });
  await chart.unmount();
});

test("a failed history read keeps the pane's error text, not the reload", async () => {
  const { useCandleChart } = await freshHook();
  const { feed, requests } = fakeFeed();
  const chart = await mountPane(useCandleChart, feed, "1h");
  const refused = Object.assign(new Error("candles unavailable"), { name: "RequestError" });

  await settle(() => requests[0]?.reply.reject(refused));

  assert.deepEqual(chartOverlay(chart.current), { kind: "error", error: refused });
  await chart.unmount();
});
