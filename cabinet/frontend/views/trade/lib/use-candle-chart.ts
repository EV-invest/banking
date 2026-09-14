"use client";

// The plotting engine's lifecycle, behind one hook: create the chart on the host once,
// load a resolution's history, fold live bars in with `series.update`, resize with the
// pane. The engine is lightweight-charts (Apache-2.0, attribution kept on); nothing
// outside this file names it, so an Advanced Charts swap is this file and a datafeed.

import { CandlestickSeries, ColorType, createChart, type IChartApi, type ISeriesApi, type UTCTimestamp } from "lightweight-charts";
import { useEffect, useRef, useState, type RefObject } from "react";

import type { CandleResolution } from "@/shared/contracts/book";
import type { Bar } from "@/views/trade/lib/candles";
import { historyWindow, type ChartFeed } from "@/views/trade/lib/chart-feed";
import { readChartPalette, type ChartPalette } from "@/views/trade/lib/theme-colors";

export type ChartState = { kind: "loading" } | { kind: "empty" } | { kind: "ready" } | { kind: "failed"; error: unknown };

// Floats are fine HERE and only here: the engine draws pixels, and the exact strings
// stay in `Bar` for the arithmetic that matters.
const toCandle = (bar: Bar) => ({ time: bar.time as UTCTimestamp, open: Number(bar.open), high: Number(bar.high), low: Number(bar.low), close: Number(bar.close) });

function chartOptions(palette: ChartPalette) {
  return {
    layout: {
      background: { type: ColorType.Solid, color: palette.background },
      textColor: palette.text,
      // Required by the library's licence; leave it on.
      attributionLogo: true,
    },
    grid: { vertLines: { color: palette.grid }, horzLines: { color: palette.grid } },
    rightPriceScale: { borderColor: palette.grid },
    timeScale: { borderColor: palette.grid, timeVisible: true, secondsVisible: false },
    crosshair: { horzLine: { labelBackgroundColor: palette.grid }, vertLine: { labelBackgroundColor: palette.grid } },
  };
}

export function useCandleChart(host: RefObject<HTMLDivElement | null>, feed: ChartFeed, resolution: CandleResolution): ChartState {
  const [state, setState] = useState<ChartState>({ kind: "loading" });
  const chart = useRef<IChartApi | null>(null);
  const series = useRef<ISeriesApi<"Candlestick"> | null>(null);

  // The engine and its one series live as long as the host does; a resolution change
  // swaps the data, not the chart, so the viewport's zoom survives it.
  useEffect(() => {
    const el = host.current;
    if (!el) return;
    const palette = readChartPalette();
    const api = createChart(el, { ...chartOptions(palette), width: el.clientWidth, height: el.clientHeight });
    const candles = api.addSeries(CandlestickSeries, {
      upColor: palette.up,
      downColor: palette.down,
      wickUpColor: palette.up,
      wickDownColor: palette.down,
      borderVisible: false,
    });
    chart.current = api;
    series.current = candles;
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
      series.current = null;
    };
  }, [host]);

  useEffect(() => {
    const candles = series.current;
    if (!candles) return;
    let cancelled = false;
    let unsubscribe: (() => void) | undefined;
    setState({ kind: "loading" });
    const { from, to } = historyWindow(resolution);
    feed
      .history(resolution, from, to)
      .then((bars) => {
        if (cancelled) return;
        candles.setData(bars.map(toCandle));
        chart.current?.timeScale().fitContent();
        setState({ kind: bars.length === 0 ? "empty" : "ready" });
        // `update` extends or replaces the last bar — never `setData` per tick.
        unsubscribe = feed.subscribe(resolution, bars[bars.length - 1] ?? null, (bar) => {
          candles.update(toCandle(bar));
          setState((s) => (s.kind === "empty" ? { kind: "ready" } : s));
        });
      })
      .catch((error: unknown) => {
        if (!cancelled) setState({ kind: "failed", error });
      });
    return () => {
      cancelled = true;
      unsubscribe?.();
    };
  }, [feed, resolution]);

  return state;
}
