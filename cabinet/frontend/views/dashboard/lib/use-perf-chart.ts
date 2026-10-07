"use client";

// The performance chart's plotting engine behind one hook: two lines on one host — the
// fund's return on the left axis, the caller's participation in USDT on the right —
// created once, refilled when the series change, resized with the card. The engine is
// lightweight-charts (Apache-2.0, attribution kept on), the same one the trade terminal
// draws candles with; nothing outside this file names it.
//
// The engine is ~50 KB gz and the dashboard's first paint is a skeleton anyway, so it is
// fetched on demand rather than shipped with the route: `preloadPerfEngine` starts the
// download while the history is still loading, and the hook mounts once it has arrived.

import type { IChartApi, ISeriesApi, UTCTimestamp } from "lightweight-charts";
import { type RefObject, useEffect, useState } from "react";

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

let engine: Promise<typeof import("lightweight-charts")> | undefined;
// One download per page load: the promise is kept, so a remount resolves from memory. A
// failed download is forgotten, so the next mount tries again instead of replaying it.
const loadEngine = () =>
  (engine ??= import("lightweight-charts").catch((error: unknown) => {
    engine = undefined;
    throw error;
  }));

/** Starts fetching the plotting engine ahead of the first plot; safe to call repeatedly. */
export function preloadPerfEngine(): void {
  // Swallowed here only: the hook's own load retries, and its rejection is the one reported.
  loadEngine().catch(() => undefined);
}

interface Plot {
  api: IChartApi;
  performance: ISeriesApi<"Line">;
  participation: ISeriesApi<"Line">;
}

export function usePerfChart(host: RefObject<HTMLDivElement | null>, series: NavSeries, format: PerfFormat): void {
  // State, not refs: the series and format effects below must run again once the engine
  // has arrived, and the arrival is asynchronous now.
  const [plot, setPlot] = useState<Plot | null>(null);

  // The engine and its two lines live as long as the host does. Scroll and scale are off:
  // this is a card on a page, and a plot that swallows a touch-drag stops the page under it
  // from scrolling on a phone — the range control is how the window changes.
  useEffect(() => {
    const el = host.current;
    if (!el) return;
    let disposed = false;
    let teardown: (() => void) | undefined;
    void loadEngine().then(({ ColorType, createChart, LineSeries }) => {
      if (disposed) return;
      const palette = readTokenColors(TOKENS, FALLBACK);
      const api = createChart(el, {
        width: el.clientWidth,
        height: el.clientHeight,
        // Transparent, because below `lg` the hero sits flat on the page rather than on a card.
        layout: { background: { type: ColorType.Solid, color: "transparent" }, textColor: palette.text, attributionLogo: true },
        grid: { vertLines: { color: palette.grid }, horzLines: { color: palette.grid } },
        leftPriceScale: { visible: true, borderColor: palette.grid },
        rightPriceScale: { borderColor: palette.grid },
        timeScale: { borderColor: palette.grid, fixLeftEdge: true, fixRightEdge: true },
        crosshair: { horzLine: { labelBackgroundColor: palette.plate }, vertLine: { labelBackgroundColor: palette.plate } },
        handleScroll: false,
        handleScale: false,
      });
      const performance = api.addSeries(LineSeries, { color: palette.performance, lineWidth: 2, priceScaleId: "left", priceLineVisible: false });
      const participation = api.addSeries(LineSeries, { color: palette.participation, lineWidth: 2, priceLineVisible: false });
      const observer = new ResizeObserver(([entry]) => {
        if (!entry) return;
        const { width, height } = entry.contentRect;
        if (width > 0 && height > 0) api.resize(width, height);
      });
      observer.observe(el);
      teardown = () => {
        observer.disconnect();
        api.remove();
      };
      setPlot({ api, performance, participation });
    });
    return () => {
      disposed = true;
      teardown?.();
    };
  }, [host]);

  useEffect(() => {
    if (!plot) return;
    plot.performance.applyOptions({ priceFormat: { type: "custom", formatter: format.performance, minMove: 0.01 } });
    plot.participation.applyOptions({ priceFormat: { type: "custom", formatter: format.participation, minMove: 0.01 } });
  }, [plot, format]);

  useEffect(() => {
    if (!plot) return;
    plot.performance.setData(series.performance.map(toPoint));
    plot.participation.setData(series.participation.map(toPoint));
    // A non-holder has no participation, and an axis with nothing on it is a column of
    // stray labels — so the right scale comes and goes with the line it belongs to.
    const held = series.participation.length > 0;
    plot.participation.applyOptions({ visible: held });
    plot.api.applyOptions({ rightPriceScale: { visible: held } });
    plot.api.timeScale().fitContent();
  }, [plot, series]);
}
