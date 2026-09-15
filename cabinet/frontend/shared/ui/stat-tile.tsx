"use client";

import { Card, Separator, Skeleton } from "@evinvest/uikit";

import { cn } from "@/shared/lib/cn";
import { TipAnchor, type TipKey } from "@/shared/tips";
import { AnimatedNumber } from "@/shared/ui/motion";

// One figure with its label and a one-line hint — the cell of a stat strip. Its own tile
// on mobile, a cell of the shared divided strip from `lg`. Lifted out of the dashboard
// once the profile page grew a strip of its own: two copies would have drifted in the
// one place a figure's weight and tone have to read the same across screens.

/** A whole-number figure, for the tiles that count rather than sum. Module-level so the
 *  count does not restart on every parent render (see `format` below). */
export const formatCount = (n: number) => String(Math.round(n));

/** The strip the tiles sit in: a 2×2 card grid on mobile, one divided row from `lg`. The
 *  caller adds the strip's own chrome (the dashboard's is flat on mobile, a card from `lg`). */
export const STAT_STRIP = "grid grid-cols-2 gap-3 lg:flex lg:flex-row lg:flex-wrap lg:items-stretch lg:gap-x-7 lg:gap-y-4 lg:px-6";

/** The hairline between two tiles of a strip, from `lg` — the mobile grid needs none. */
export function StatDivider() {
  return <Separator orientation="vertical" className="hidden self-stretch lg:block" />;
}

// Takes the figure and its formatter rather than a finished string: a string can
// only be swapped, and swapping is the thing AnimatedNumber exists to replace.
// `format` has to be a stable reference (all of these are module functions from
// shared/lib/money) or the count restarts on every parent render.
//
// `value: null` is "not here yet" and draws a skeleton; `unavailable` is "the read
// failed" and draws a dash — a figure that could not be read must not render as a zero.
export function StatTile({ label, value, format, tone, hint, tip, unavailable }: { label: string; value: number | null; format: (n: number) => string; tone?: "gain" | "loss"; hint: string; tip?: TipKey; unavailable?: boolean }) {
  const valueClass = tone === "gain" ? "text-positive" : tone === "loss" ? "text-accent-error" : "text-ink";
  const hintClass = tone === "gain" ? "text-positive/80" : tone === "loss" ? "text-accent-error/80" : "text-ink-soft";
  return (
    <Card className="min-w-0 flex-1 gap-1 px-3.5 py-3 lg:min-w-30 lg:gap-1.5 lg:rounded-none lg:border-0 lg:bg-transparent lg:p-0 lg:shadow-none">
      <div className="flex items-center gap-1.5">
        <p className="truncate text-xs font-medium text-ink-soft">{label}</p>
        {tip && <TipAnchor anchor={tip} />}
      </div>
      {unavailable ? (
        <p className="text-xl font-semibold tabular-nums text-ink-soft lg:text-2xl">—</p>
      ) : value === null ? (
        <Skeleton className="h-6 w-20" />
      ) : (
        <p className={cn("truncate text-xl font-semibold tabular-nums lg:text-2xl", valueClass)}><AnimatedNumber value={value} format={format} /></p>
      )}
      <p className={cn("truncate text-xs font-medium", hintClass)}>{hint}</p>
    </Card>
  );
}
