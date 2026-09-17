"use client";

import { Card, Separator, Skeleton } from "@evinvest/uikit";
import type { ReactNode } from "react";

import { cn } from "@/shared/lib/cn";
import { type Valence, VALENCE_CLASS } from "@/shared/lib/money";
import { TipAnchor, type TipKey } from "@/shared/tips";
import { AnimatedNumber } from "@/shared/ui/motion";

// One figure with its label and a one-line hint — the cabinet's stat tile. Lifted out of
// the dashboard once the profile page grew a strip of its own, and then took in the invest
// page's boxed tile: two copies would have drifted in the one place a figure's weight and
// tone have to read the same across screens. The uikit's `Stat` is an inline "figure beside
// its label" span for a landing band, not a tile, so it is not the base here.

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

// `strip`: a cell of the shared divided strip from `lg`, its own card below it. `box`: a
// tile that stays boxed at every width — the product page's holding figures, in a grid.
const VARIANT = {
  strip: "min-w-0 flex-1 gap-1 px-3.5 py-3 lg:min-w-30 lg:gap-1.5 lg:rounded-none lg:border-0 lg:bg-transparent lg:p-0 lg:shadow-none",
  box: "gap-1 rounded-lg bg-secondary px-3 py-3 shadow-none",
} as const;

// The figure is either a number with its formatter — AnimatedNumber counts it in, and a
// string could only be swapped — or a string already formatted for a unit the count cannot
// carry (units and a NAV beside a "USDT" suffix). `format` has to be a stable reference (all
// of these are module functions from shared/lib/money) or the count restarts on every
// parent render.
type Counted = { value: number | null; format: (n: number) => string };
type Figure = Counted | { value: string; format?: never };

// A predicate rather than `typeof props.value` inline: the arms differ in a non-literal
// property, which control flow does not narrow on.
const isCounted = (f: Figure): f is Counted => typeof f.value !== "string";

interface StatTileProps {
  label: string;
  /** The figure's valence (`valence()` from shared/lib/money): a gain and a loss take the
   *  one pair every investor screen uses; a flat figure, or one that has none, stays in ink. */
  tone?: Valence;
  hint?: string;
  tip?: TipKey;
  /** A mark before the figure — the trend arrow on a P&L. */
  icon?: ReactNode;
  /** The tile the reader came for, one step larger than its neighbours. */
  emphasis?: boolean;
  /** "the read failed" — draws a dash. A figure that could not be read must not render as a zero. */
  unavailable?: boolean;
  variant?: keyof typeof VARIANT;
}

// `value: null` is "not here yet" and draws a skeleton; `unavailable` is "the read failed"
// and draws a dash.
export function StatTile(props: StatTileProps & Figure) {
  const { label, tone, hint, tip, icon, emphasis, unavailable, variant = "strip" } = props;
  const loading = isCounted(props) && props.value === null;
  const figure = isCounted(props) ? props.value !== null && <AnimatedNumber value={props.value} format={props.format} /> : props.value;
  const valueClass = VALENCE_CLASS[tone ?? "flat"];
  const hintClass = tone === "gain" ? "text-positive/80" : tone === "loss" ? "text-accent-error/80" : "text-ink-soft";
  // The strip's figures are all the same size; the box marks the one that matters.
  const figureClass = variant === "strip" ? "text-xl font-semibold lg:text-2xl" : emphasis ? "text-xl font-semibold" : "text-base";
  return (
    <Card className={VARIANT[variant]}>
      <div className="flex items-center gap-1.5">
        <p className="truncate text-xs font-medium text-ink-soft">{label}</p>
        {tip && <TipAnchor anchor={tip} />}
      </div>
      {unavailable ? (
        <p className={cn("tabular-nums text-ink-soft", figureClass)}>—</p>
      ) : loading ? (
        <Skeleton className="h-6 w-20" />
      ) : (
        <p className={cn("truncate tabular-nums", figureClass, valueClass)}>
          {/* Inline rather than a flex row, so a long figure still truncates as text. */}
          {icon && <span className="mr-1 inline-flex align-middle">{icon}</span>}
          {figure}
        </p>
      )}
      {hint && <p className={cn("truncate text-xs font-medium", hintClass)}>{hint}</p>}
    </Card>
  );
}
