// The chart's data shapes and the one piece of arithmetic the live update needs: which
// bucket a trade falls into, and what it does to that bucket's bar. Pure and chart-agnostic
// — `ChartFeed` (./chart-feed) produces these, `use-candle-chart` hands them to the
// plotting engine, and neither the engine nor the wire shape leaks across.
//
// Prices are compared as decimal strings through exact base units, never as floats: a
// high/low decided by `Number()` on an 18-decimal price is a wick that lies.

import type { Candle, CandleResolution, Trade } from "@/shared/contracts/book";

import { RESOLUTION_SECONDS } from "../../../entities/book/lib/vocabulary.ts";
import { fromBaseUnits, toBaseUnits } from "../../../shared/lib/money.ts";

/** One OHLCV bar as the chart consumes it. `time` is the bucket start in unix seconds. */
export interface Bar {
  time: number;
  open: string;
  high: string;
  low: string;
  close: string;
  volume: string;
}

const seconds = (value: number | string | undefined): number => {
  const n = Number(value ?? 0);
  return Number.isFinite(n) ? Math.floor(n) : 0;
};

/** The start of the bucket a moment falls into. */
export function bucketStart(at: number, resolution: CandleResolution): number {
  const size = RESOLUTION_SECONDS[resolution];
  return Math.floor(at / size) * size;
}

/** A wire candle → a bar. Buckets with no trades are dropped: the engine draws gaps as
 *  gaps, and a zero-priced bar would draw a wick to the floor. */
export function barFromCandle(candle: Candle): Bar | null {
  if (!candle.close || !candle.open) return null;
  return {
    time: seconds(candle.time),
    open: candle.open,
    high: candle.high ?? candle.open,
    low: candle.low ?? candle.open,
    close: candle.close,
    volume: candle.volume ?? "0",
  };
}

const max = (a: string, b: string): string => (toBaseUnits(a) >= toBaseUnits(b) ? a : b);
const min = (a: string, b: string): string => (toBaseUnits(a) <= toBaseUnits(b) ? a : b);

/**
 * What a trade does to the bar it falls into.
 *
 * Same bucket as `last`: the close moves, the high/low widen, the volume grows. A later
 * bucket: a new bar opens at the trade's price. An EARLIER bucket — a trade delivered
 * after the bar it belongs to has closed — is ignored rather than rewriting history: the
 * next history fetch carries the hub's own aggregate, which is the authority.
 */
export function applyTrade(last: Bar | null, trade: Trade, resolution: CandleResolution): Bar | null {
  if (!trade.price) return null;
  const at = bucketStart(seconds(trade.executed_at), resolution);
  const size = trade.size ?? "0";
  if (last === null || at > last.time) {
    return { time: at, open: trade.price, high: trade.price, low: trade.price, close: trade.price, volume: size };
  }
  if (at < last.time) return null;
  return {
    ...last,
    high: max(last.high, trade.price),
    low: min(last.low, trade.price),
    close: trade.price,
    volume: fromBaseUnits(toBaseUnits(last.volume) + toBaseUnits(size)),
  };
}

/** How far back one screenful of history reaches — enough bars to fill a chart. */
export const HISTORY_BARS = 300;

export function historyFrom(now: number, resolution: CandleResolution): number {
  return bucketStart(now, resolution) - RESOLUTION_SECONDS[resolution] * HISTORY_BARS;
}
