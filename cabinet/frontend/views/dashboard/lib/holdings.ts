// What the dashboard reads off the caller's positions: the figures for the stat strip and
// the hero badge, and the slices of the "What I own" card. Pure, so the view only composes.

import type { Position } from "@/shared/contracts";
import { num } from "@/shared/lib/money";

// Allocation slices cycle the chart palette — distinguishable hues carrying no
// significance. The bar names its rung twice because Progress paints track and indicator
// from `--primary`, and the child selector is the only way to reach the indicator without
// forking the component.
const ACCENTS = [
  { dot: "bg-chart-1", bar: "bg-chart-1/20 *:bg-chart-1" },
  { dot: "bg-chart-2", bar: "bg-chart-2/20 *:bg-chart-2" },
  { dot: "bg-chart-3", bar: "bg-chart-3/20 *:bg-chart-3" },
  { dot: "bg-chart-4", bar: "bg-chart-4/20 *:bg-chart-4" },
] as const;

export type Accent = (typeof ACCENTS)[number];

export interface Allocation {
  name: string;
  value: number;
  accent: Accent;
}

export interface HoldingsSummary {
  /** Unrealised P&L across every position, in the dashboard's summary money. */
  pnl: number;
  /** What the caller put in, at cost basis. */
  netContributed: number;
  /** The all-time return as a percentage, or `null` off a zero cost basis — undefined, not 0%. */
  allTimePct: number | null;
}

export function summariseHoldings(positions: readonly Position[]): HoldingsSummary {
  const pnl = positions.reduce((s, p) => s + num(p.pnl), 0);
  const netContributed = positions.reduce((s, p) => s + num(p.cost_basis), 0);
  return { pnl, netContributed, allTimePct: netContributed > 0 ? (pnl / netContributed) * 100 : null };
}

/** One slice per position, named by the catalog rather than the slug that keys it. */
export function toAllocations(positions: readonly Position[], titleOf: (service: string | undefined) => string): Allocation[] {
  return positions.map((p, i) => ({ name: titleOf(p.service), value: num(p.value), accent: ACCENTS[i % ACCENTS.length]! }));
}
