"use client";

// The performance chart's plotting engine behind one hook: two lines on one host — the
// fund's return on the left axis, the caller's participation in USDT on the right —
// created once, refilled when the series change, resized with the card. The engine is
// lightweight-charts (Apache-2.0, attribution kept on), the same one the trade terminal
// draws candles with; nothing outside this file names it.
//
// The engine is ~50 KB gz and the dashboard's first paint is a skeleton anyway, so it is
// fetched on demand rather than shipped with the route: `preloadPerfEngine` starts the
// download while the history is still loading. The hook reports where that download is
// (`PerfEngineState`) so the plot can keep its skeleton until the lines are drawn.

import type { IChartApi, ISeriesApi, UTCTimestamp } from "lightweight-charts";
import { type RefObject, useEffect, useLayoutEffect, useRef, useSyncExternalStore } from "react";

import type { NavSeries, SeriesPoint } from "@/entities/fund/lib/nav-series";
import { ENGINE_FALLBACK, readTokenColors } from "@/shared/lib/chart-palette";

/** Axis labels, in the reader's locale. One per unit of measure, as `shared/lib/money.ts` has it. */
export interface PerfFormat {
  performance: (pct: number) => string;
  participation: (usdt: number) => string;
}

// The legend's dots and these lines are the same two tokens (`chart-3`, `chart-2`); text
// and grid are the card's. The crosshair's labels sit on a SOLID plate: `--color-border`
// is ink at 14 % alpha, and over the dark page the engine would pick dark text for it.
const TOKENS = { text: "--color-ink-soft", grid: "--color-border", plate: "--color-secondary", performance: "--color-chart-3", participation: "--color-chart-2" };
const FALLBACK = { text: ENGINE_FALLBACK.text, grid: ENGINE_FALLBACK.grid, plate: ENGINE_FALLBACK.plate, performance: ENGINE_FALLBACK.yellow, participation: ENGINE_FALLBACK.green };

const toPoint = (p: SeriesPoint) => ({ time: p.time as UTCTimestamp, value: p.value });

type Engine = typeof import("lightweight-charts");

/** Where the plot stands: still waiting for the engine, drawn, or never going to be. */
export type PerfEngineState = { kind: "loading" } | { kind: "ready" } | { kind: "failed"; error: unknown };

// The download's progress is an external store rather than component state: it outlives
// any one plot (the preload starts it before there is one), and reading it through
// `useSyncExternalStore` lets the chart be created in a layout effect the same commit the
// engine is known to be here, with no state set from inside an effect.
type Status = { kind: "loading" } | { kind: "ready"; engine: Engine } | { kind: "failed"; error: unknown };
const LOADING: Status = { kind: "loading" };
let status: Status = LOADING;
const listeners = new Set<() => void>();
const publish = (next: Status) => {
  status = next;
  listeners.forEach((l) => l());
};
const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => void listeners.delete(listener);
};
const snapshot = () => status;
const serverSnapshot = () => LOADING;

let download: Promise<Engine> | undefined;
// One download per page load: a settled one is reused by every later plot. A failed one is
// forgotten, so the next mount asks again rather than replaying it.
const loadEngine = (): Promise<Engine> => {
  if (download) return download;
  if (status.kind === "failed") publish(LOADING);
  download = import("lightweight-charts").then(
    (engine) => {
      publish({ kind: "ready", engine });
      return engine;
    },
    (error: unknown) => {
      download = undefined;
      publish({ kind: "failed", error });
      throw error;
    },
  );
  return download;
};

/** Starts fetching the plotting engine ahead of the first plot; safe to call repeatedly. */
export function preloadPerfEngine(): void {
  // The failure is not lost: it is published to every plot as `failed`.
  loadEngine().catch(() => undefined);
}

interface Plot {
  api: IChartApi;
  performance: ISeriesApi<"Line">;
  participation: ISeriesApi<"Line">;
}

function fill(plot: Plot, series: NavSeries): void {
  plot.performance.setData(series.performance.map(toPoint));
  plot.participation.setData(series.participation.map(toPoint));
  // A non-holder has no participation, and an axis with nothing on it is a column of
  // stray labels — so the right scale comes and goes with the line it belongs to.
  const held = series.participation.length > 0;
  plot.participation.applyOptions({ visible: held });
  plot.api.applyOptions({ rightPriceScale: { visible: held } });
  plot.api.timeScale().fitContent();
}

function applyFormat(plot: Plot, format: PerfFormat): void {
  plot.performance.applyOptions({ priceFormat: { type: "custom", formatter: format.performance, minMove: 0.01 } });
  plot.participation.applyOptions({ priceFormat: { type: "custom", formatter: format.participation, minMove: 0.01 } });
}

const READY: PerfEngineState = { kind: "ready" };
const toState = (s: Status): PerfEngineState => (s.kind === "ready" ? READY : s);

/** The download's status, and the download itself started (or retried) by the first reader. */
function useEngineStatus(): Status {
  const current = useSyncExternalStore(subscribe, snapshot, serverSnapshot);
  useEffect(() => {
    loadEngine().catch(() => undefined);
  }, []);
  return current;
}

/** Whether the plot can be drawn yet — for the surface that decides when to show it. */
export function usePerfEngine(): PerfEngineState {
  return toState(useEngineStatus());
}

export function usePerfChart(host: RefObject<HTMLDivElement | null>, series: NavSeries, format: PerfFormat): PerfEngineState {
  const current = useEngineStatus();
  const engine = current.kind === "ready" ? current.engine : null;
  const plot = useRef<Plot | null>(null);
  // What the creating effect draws with. Kept current by the effects below rather than
  // read from a closure, so creation does not re-run on every new series.
  const latest = useRef({ series, format });

  // The engine and its two lines live as long as the host does. Created, formatted and
  // filled in one layout pass, so no frame shows an empty or half-drawn plot. Scroll and
  // scale are off: this is a card on a page, and a plot that swallows a touch-drag stops
  // the page under it from scrolling on a phone — the range control is how the window changes.
  useLayoutEffect(() => {
    const el = host.current;
    if (!el || !engine) return;
    const palette = readTokenColors(TOKENS, FALLBACK);
    const api = engine.createChart(el, {
      width: el.clientWidth,
      height: el.clientHeight,
      // Transparent, because below `lg` the hero sits flat on the page rather than on a card.
      layout: { background: { type: engine.ColorType.Solid, color: "transparent" }, textColor: palette.text, attributionLogo: true },
      grid: { vertLines: { color: palette.grid }, horzLines: { color: palette.grid } },
      leftPriceScale: { visible: true, borderColor: palette.grid },
      rightPriceScale: { borderColor: palette.grid },
      timeScale: { borderColor: palette.grid, fixLeftEdge: true, fixRightEdge: true },
      crosshair: { horzLine: { labelBackgroundColor: palette.plate }, vertLine: { labelBackgroundColor: palette.plate } },
      handleScroll: false,
      handleScale: false,
    });
    const created: Plot = {
      api,
      performance: api.addSeries(engine.LineSeries, { color: palette.performance, lineWidth: 2, priceScaleId: "left", priceLineVisible: false }),
      participation: api.addSeries(engine.LineSeries, { color: palette.participation, lineWidth: 2, priceLineVisible: false }),
    };
    applyFormat(created, latest.current.format);
    fill(created, latest.current.series);
    plot.current = created;
    const observer = new ResizeObserver(([entry]) => {
      if (!entry) return;
      const { width, height } = entry.contentRect;
      if (width > 0 && height > 0) api.resize(width, height);
    });
    observer.observe(el);
    return () => {
      observer.disconnect();
      api.remove();
      plot.current = null;
    };
  }, [host, engine]);

  // Later changes, applied to the plot that is already there — skipped when creation has
  // just drawn the same value. Layout effects, for the same no-stale-frame reason.
  useLayoutEffect(() => {
    if (latest.current.format === format) return;
    latest.current.format = format;
    if (plot.current) applyFormat(plot.current, format);
  }, [format]);

  useLayoutEffect(() => {
    if (latest.current.series === series) return;
    latest.current.series = series;
    if (plot.current) fill(plot.current, series);
  }, [series]);

  return toState(current);
}
