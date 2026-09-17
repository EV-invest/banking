"use client";

// The performance chart's plotting engine behind one hook: two lines on one host — the
// fund's return on the left axis, the caller's participation in USDT on the right —
// created once, refilled when the series change, resized with the card. The engine is
// lightweight-charts (Apache-2.0, attribution kept on), the same one the trade terminal
// draws candles with; nothing outside this file names it.

import { ColorType, createChart, type IChartApi, type ISeriesApi, LineSeries, type UTCTimestamp } from "lightweight-charts";
import { type RefObject, useEffect, useRef } from "react";

import type { NavSeries, SeriesPoint } from "@/entities/fund/lib/nav-series";
import { readTokenColors } from "@/shared/lib/chart-palette";

/** Axis labels, in the reader's locale. One per unit of measure, as `shared/lib/money.ts` has it. */
export interface PerfFormat {
  performance: (pct: number) => string;
  participation: (usdt: number) => string;
}

// The legend's dots and these lines are the same two tokens (`chart-3`, `chart-2`);
// text and grid are the card's. Fallbacks are deliberately dull, so a token the probe
// could not resolve reads as "not themed" rather than as a design choice.
const TOKENS = { text: "--color-ink-soft", grid: "--color-border", performance: "--color-chart-3", participation: "--color-chart-2" };
const FALLBACK = { text: "#9a9a9a", grid: "#2a2a2a", performance: "#c9a227", participation: "#2e9e5b" };

const toPoint = (p: SeriesPoint) => ({ time: p.time as UTCTimestamp, value: p.value });

export function usePerfChart(host: RefObject<HTMLDivElement | null>, series: NavSeries, format: PerfFormat): void {
  const chart = useRef<IChartApi | null>(null);
  const performance = useRef<ISeriesApi<"Line"> | null>(null);
  const participation = useRef<ISeriesApi<"Line"> | null>(null);

  // The engine and its two lines live as long as the host does. Scroll and scale are off:
  // this is a card on a page, and a plot that swallows a touch-drag stops the page under it
  // from scrolling on a phone — the range control is how the window changes.
  useEffect(() => {
    const el = host.current;
    if (!el) return;
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
      crosshair: { horzLine: { labelBackgroundColor: palette.grid }, vertLine: { labelBackgroundColor: palette.grid } },
      handleScroll: false,
      handleScale: false,
    });
    performance.current = api.addSeries(LineSeries, { color: palette.performance, lineWidth: 2, priceScaleId: "left", priceLineVisible: false });
    participation.current = api.addSeries(LineSeries, { color: palette.participation, lineWidth: 2, priceLineVisible: false });
    chart.current = api;
    const observer = new ResizeObserver(([entry]) => {
      if (!entry) return;
      const { width, height } = entry.contentRect;
      if (width > 0 && height > 0) api.resize(width, height);
    });
    observer.observe(el);
    return () => {
      observer.disconnect();
      api.remove();
      chart.current = null;
      performance.current = null;
      participation.current = null;
    };
  }, [host]);

  useEffect(() => {
    performance.current?.applyOptions({ priceFormat: { type: "custom", formatter: format.performance, minMove: 0.01 } });
    participation.current?.applyOptions({ priceFormat: { type: "custom", formatter: format.participation, minMove: 0.01 } });
  }, [format]);

  useEffect(() => {
    const api = chart.current;
    if (!api || !performance.current || !participation.current) return;
    performance.current.setData(series.performance.map(toPoint));
    participation.current.setData(series.participation.map(toPoint));
    // A non-holder has no participation, and an axis with nothing on it is a column of
    // stray labels — so the right scale comes and goes with the line it belongs to.
    const held = series.participation.length > 0;
    participation.current.applyOptions({ visible: held });
    api.applyOptions({ rightPriceScale: { visible: held } });
    api.timeScale().fitContent();
  }, [series]);
}
