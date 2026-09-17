// The shape of the performance chart's two series, from the wire to the plot.
//
// Everything on `FundNavHistory` arrives as strings — decimals and unix-second stamps
// alike — and the plotting engine wants sorted, unique, numeric points. Floats are fine
// HERE and only here: the engine draws pixels, and no figure derived in this module goes
// back into money math (`shared/lib/money.ts` keeps that rule).
//
// Deliberately import-free (types only) so the Node test runner can load it without the
// `@/` alias.

import type { FundNavHistory } from "@/shared/contracts";

/** One plotted point: unix seconds and the value the engine draws at that instant. */
export interface SeriesPoint {
  time: number;
  value: number;
}

export interface NavSeries {
  /** The fund's return since the first mark in the window, in percent (0 at that mark). */
  performance: SeriesPoint[];
  /** The caller's holding valued at each instant, in USDT. Empty for a non-holder. */
  participation: SeriesPoint[];
  /** The NAV the performance series is rebased on — what "0 %" means on the left axis. */
  baseNav: number | null;
}

// The four ranges. `1M`/`6M`/`1Y` are near-universal abbreviations but still travel as
// keys — a locale that spells its months differently should be able to say so.
export const HISTORY_RANGES = ["1m", "6m", "1y", "all"] as const;
export type HistoryRange = (typeof HISTORY_RANGES)[number];

/** Whether a control's reported value names a range — the kit's toggle group reports "" for a press on the active item. */
export function isHistoryRange(value: unknown): value is HistoryRange {
  return typeof value === "string" && (HISTORY_RANGES as readonly string[]).includes(value);
}

// Fixed day counts rather than calendar months: "31 March minus one month" has no single
// answer, and a window that lands on a different day depending on today's date would make
// the same range key two different cache entries across midnight.
const RANGE_DAYS: Readonly<Record<Exclude<HistoryRange, "all">, number>> = { "1m": 30, "6m": 182, "1y": 365 };

const DAY_S = 24 * 60 * 60;

/**
 * The window's `from` bound for a range, in unix seconds — or `undefined` for all-time,
 * which the route reads as "from the first mark".
 *
 * Floored to the start of a UTC day so the value — and with it the resource's cache key —
 * is the same on every render of the same day, rather than a fresh key every second.
 */
export function rangeFrom(range: HistoryRange, nowMs: number): number | undefined {
  if (range === "all") return undefined;
  const nowS = Math.floor(nowMs / 1000);
  return Math.floor((nowS - RANGE_DAYS[range] * DAY_S) / DAY_S) * DAY_S;
}

/**
 * A wire stamp (a decimal string of unix seconds, or a number) → seconds, or `null` for
 * anything that is not a positive finite number. `0` is the contract's "no stamp".
 */
function toSeconds(stamp: number | string | undefined): number | null {
  if (stamp === undefined) return null;
  const seconds = Number(stamp);
  return Number.isFinite(seconds) && seconds > 0 ? seconds : null;
}

function toValue(decimal: string | undefined): number | null {
  if (decimal === undefined) return null;
  const value = Number(decimal);
  return Number.isFinite(value) ? value : null;
}

/**
 * Sorted ascending and unique by `time`, which is what the engine requires. Two points at
 * the same second keep the LATER one in wire order: the wire is oldest-first, so the later
 * entry is the one posted last.
 */
function tidy(points: SeriesPoint[]): SeriesPoint[] {
  const byTime = new Map<number, number>();
  for (const point of points) byTime.set(point.time, point.value);
  return [...byTime.entries()].sort((a, b) => a[0] - b[0]).map(([time, value]) => ({ time, value }));
}

/**
 * The two plotted series of one history, or empty series where the wire has nothing.
 *
 * The performance line is rebased on the OLDEST mark received — which, for a `truncated`
 * history, is the oldest mark the hub kept rather than the oldest in the window. The note
 * the chart shows for `truncated` (`dash.historyTruncated`) says so.
 */
export function toNavSeries(history: FundNavHistory | undefined): NavSeries {
  const marks: SeriesPoint[] = [];
  for (const mark of history?.marks ?? []) {
    const time = toSeconds(mark.posted_at);
    const nav = toValue(mark.nav);
    if (time !== null && nav !== null && nav > 0) marks.push({ time, value: nav });
  }
  const navs = tidy(marks);
  const baseNav = navs[0]?.value ?? null;
  const performance = baseNav === null ? [] : navs.map(({ time, value }) => ({ time, value: (value / baseNav - 1) * 100 }));

  const participation: SeriesPoint[] = [];
  for (const point of history?.participation ?? []) {
    const time = toSeconds(point.at);
    const value = toValue(point.value);
    if (time !== null && value !== null) participation.push({ time, value });
  }
  return { performance, participation: tidy(participation), baseNav };
}
