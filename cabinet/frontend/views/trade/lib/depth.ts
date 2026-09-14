// The depth bars behind the book: each level's cumulative size, and that total as a
// fraction of the deepest level shown. Exact bigint arithmetic on the wire's decimal
// strings — a book is a money surface, and a float that rounds two levels to the same
// total would draw a bar that lies.
//
// Relative import of the money module so `node --test` can load this without the `@/`
// alias, as `views/admin/payments/lib/terms.ts` does.

import type { BookLevel } from "@/shared/contracts/book";

import { fromBaseUnits, toBaseUnits } from "../../../shared/lib/money.ts";

export interface DepthRow {
  price: string;
  size: string;
  /** Cumulative units from the best level down to and including this one. */
  total: string;
  /** `total / max(total across both sides)`, 0..1 — the width of the bar. */
  depth: number;
  orders: number;
}

/** The bar is a CSS percentage; this is the resolution it is computed at. */
const DEPTH_SCALE = 10_000n;

function accumulate(levels: readonly BookLevel[]): { rows: Omit<DepthRow, "depth">[]; totals: bigint[] } {
  const rows: Omit<DepthRow, "depth">[] = [];
  const totals: bigint[] = [];
  let running = 0n;
  for (const level of levels) {
    running += toBaseUnits(level.size);
    totals.push(running);
    rows.push({ price: level.price ?? "", size: level.size ?? "0", total: fromBaseUnits(running), orders: level.orders ?? 0 });
  }
  return { rows, totals };
}

/**
 * Both sides at once, so the bars are drawn against ONE scale: a bid side three times
 * deeper than the ask side should look three times deeper, which per-side scaling hides.
 * Levels arrive best-first (`bids` highest, `asks` lowest) and come back in that order.
 */
export function depthRows(bids: readonly BookLevel[] | undefined, asks: readonly BookLevel[] | undefined): { bids: DepthRow[]; asks: DepthRow[] } {
  const b = accumulate(bids ?? []);
  const a = accumulate(asks ?? []);
  const deepest = [...b.totals, ...a.totals].reduce((max, t) => (t > max ? t : max), 0n);
  const fraction = (total: bigint): number => (deepest === 0n ? 0 : Number((total * DEPTH_SCALE) / deepest) / Number(DEPTH_SCALE));
  return {
    bids: b.rows.map((row, i) => ({ ...row, depth: fraction(b.totals[i] ?? 0n) })),
    asks: a.rows.map((row, i) => ({ ...row, depth: fraction(a.totals[i] ?? 0n) })),
  };
}
