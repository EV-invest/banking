"use client";

// The plotting engine's lifecycle, behind one hook: create the chart on the host once,
// load a resolution's history, fold live bars in with `series.update`, resize with the
// pane. The engine is lightweight-charts (Apache-2.0, attribution kept on); nothing
// outside this file names it, so an Advanced Charts swap is this file and a datafeed.
//
// The engine is ~50 KB gz, so it is fetched on demand rather than shipped with the route;
// until it arrives the pane shows the same "loading" state as while history is in flight.

import type { IChartApi, ISeriesApi, UTCTimestamp } from "lightweight-charts";
import { useEffect, useState, type RefObject } from "react";

import type { CandleResolution } from "@/shared/contracts/book";
import type { Bar } from "@/views/trade/lib/candles";
import { historyWindow, type ChartFeed } from "@/views/trade/lib/chart-feed";
import { readChartPalette, type ChartPalette } from "@/views/trade/lib/theme-colors";

export type ChartState = { kind: "loading" } | { kind: "empty" } | { kind: "ready" } | { kind: "failed"; error: unknown };

// Floats are fine HERE and only here: the engine draws pixels, and the exact strings
// stay in `Bar` for the arithmetic that matters.
const toCandle = (bar: Bar) => ({ time: bar.time as UTCTimestamp, open: Number(bar.open), high: Number(bar.high), low: Number(bar.low), close: Number(bar.close) });

type Engine = typeof import("lightweight-charts");

let engine: Promise<Engine> | undefined;
// One download per page load; a failed one is forgotten so the next mount tries again.
const loadEngine = () =>
  (engine ??= import("lightweight-charts").catch((error: unknown) => {
    engine = undefined;
    throw error;
  }));

function chartOptions(palette: ChartPalette, { ColorType }: Engine) {
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

interface Plot {
  api: IChartApi;
  candles: ISeriesApi<"Candlestick">;
}

/** A load's outcome, stamped with what it was loaded for. */
interface Outcome {
  feed: ChartFeed;
  resolution: CandleResolution;
  state: ChartState;
}

const LOADING: ChartState = { kind: "loading" };

export function useCandleChart(host: RefObject<HTMLDivElement | null>, feed: ChartFeed, resolution: CandleResolution): ChartState {
  // State, not refs: the data effect must run again once the engine has arrived.
  const [plot, setPlot] = useState<Plot | null>(null);
  const [engineError, setEngineError] = useState<{ error: unknown } | null>(null);
  // "Loading" is derived rather than set: an outcome stamped with another feed or
  // resolution is stale, so a switch reads as loading from its first render on.
  const [outcome, setOutcome] = useState<Outcome | null>(null);

  // The engine and its one series live as long as the host does; a resolution change
  // swaps the data, not the chart, so the viewport's zoom survives it.
  useEffect(() => {
    const el = host.current;
    if (!el) return;
    let disposed = false;
    let teardown: (() => void) | undefined;
    loadEngine()
      .then((lc) => {
        if (disposed) return;
        const palette = readChartPalette();
        const api = lc.createChart(el, { ...chartOptions(palette, lc), width: el.clientWidth, height: el.clientHeight });
        const candles = api.addSeries(lc.CandlestickSeries, {
          upColor: palette.up,
          downColor: palette.down,
          wickUpColor: palette.up,
          wickDownColor: palette.down,
          borderVisible: false,
        });
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
        setPlot({ api, candles });
      })
      .catch((error: unknown) => {
        // The pane already renders a failed state; an engine that never arrived is one.
        if (!disposed) setEngineError({ error });
      });
    return () => {
      disposed = true;
      teardown?.();
    };
  }, [host]);

  useEffect(() => {
    if (!plot) return;
    const { api, candles } = plot;
    let cancelled = false;
    let unsubscribe: (() => void) | undefined;
    const settle = (state: ChartState) => setOutcome({ feed, resolution, state });
    const { from, to } = historyWindow(resolution);
    feed
      .history(resolution, from, to)
      .then((bars) => {
        if (cancelled) return;
        candles.setData(bars.map(toCandle));
        api.timeScale().fitContent();
        settle({ kind: bars.length === 0 ? "empty" : "ready" });
        // `update` extends or replaces the last bar — never `setData` per tick.
        unsubscribe = feed.subscribe(resolution, bars[bars.length - 1] ?? null, (bar) => {
          candles.update(toCandle(bar));
          setOutcome((o) => (o?.state.kind === "empty" ? { ...o, state: { kind: "ready" } } : o));
        });
      })
      .catch((error: unknown) => {
        if (!cancelled) settle({ kind: "failed", error });
      });
    return () => {
      cancelled = true;
      unsubscribe?.();
    };
  }, [plot, feed, resolution]);

  if (engineError) return { kind: "failed", error: engineError.error };
  return outcome && outcome.feed === feed && outcome.resolution === resolution ? outcome.state : LOADING;
}
