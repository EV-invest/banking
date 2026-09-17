"use client";

// The hero of the dashboard: the portfolio value, its all-time badge, and the performance
// chart of the allocation the caller holds. Mobile (Figma `cabinet/mobile/home`) reads it
// as page content, not as a card: the value sits flat on the background, the range switch
// spans the width below it, and only the plot is boxed — with its legend above. From `lg`
// the whole block is the desktop card again.

import { Minus, TrendingDown, TrendingUp } from "lucide-react";
import { useLocale, useT } from "@evinvest/i18n/react";
import { Badge, Card, CardContent, Skeleton } from "@evinvest/uikit";
import { useCallback, useState } from "react";

import { HISTORY_RANGES, type HistoryRange, rangeFrom } from "@/entities/fund/lib/nav-series";
import { cn } from "@/shared/lib/cn";
import { AnimatedNumber, StaggerItem } from "@/shared/ui/motion";
import { TipAnchor } from "@/shared/tips";
import { CARD_FROM_LG } from "@/views/dashboard/lib/chrome";
import { formatPct, formatUsd, num } from "@/views/dashboard/lib/format";
import { PerfChart } from "@/views/dashboard/ui/perf-chart";

// i18n-max: 4 — four equal columns of a grid segmented control on mobile.
const RANGE_LABEL_KEYS: Readonly<Record<HistoryRange, string>> = {
  "1m": "dash.range.1m",
  "6m": "dash.range.6m",
  "1y": "dash.range.1y",
  all: "dash.range.all",
};

export interface PerfCardProps {
  value: string | undefined;
  loading: boolean;
  allTimePct: number | null;
  /** The allocation the chart draws; `null` while the dashboard is still resolving which. */
  allocation: string | null;
  className?: string;
}

export function PerfCard({ value, loading, allTimePct, allocation, className }: PerfCardProps) {
  const t = useT();
  const locale = useLocale();
  // Bound once per locale: AnimatedNumber restarts its count whenever the identity of
  // `format` changes, and a fresh closure every render would restart it every render.
  const usd = useCallback((n: number) => formatUsd(n, locale), [locale]);
  // The window's lower bound is fixed when the range is picked, not derived on every
  // render: the clock is read in the click handler, and "all" — the initial range — needs
  // no clock at all.
  const [span, setSpan] = useState<{ range: HistoryRange; from: number | undefined }>({ range: "all", from: undefined });
  // Three tones, not two: a flat all-time return is neither a gain nor a loss, and an
  // upward arrow on "+0.0%" claims a gain that did not happen — same rule as StatTile.
  const trend = allTimePct === null || allTimePct === 0 ? "flat" : allTimePct < 0 ? "down" : "up";
  return (
    // From `xl` the hero spans both rows of the side column and fills them — otherwise the
    // plot keeps its natural height and leaves a gap under the card whenever the side
    // column is the taller of the two. A `StaggerItem` rather than a wrapped `Card` because
    // that `xl:h-full` — and the row-span it fills — are the parent's business.
    <StaggerItem as={Card} className={cn("flex-1 gap-4 lg:gap-5 xl:h-full", CARD_FROM_LG, className)}>
      <div className="flex flex-col gap-3.5 lg:flex-row lg:items-start lg:justify-between lg:gap-4 lg:px-6">
        <div className="flex min-w-0 flex-col gap-2">
          <p className="flex items-center gap-1.5 text-xs font-medium uppercase tracking-wider text-primary-ink">
            {t("dash.portfolioValue")}
            <TipAnchor anchor="dashboard.performance.portfolio-value" />
          </p>
          <div className="flex flex-col items-start gap-2.5 lg:flex-row lg:items-center lg:gap-3.5">
            {loading ? <Skeleton className="h-10 w-40 lg:h-12 lg:w-48" /> : <p className="text-4xl font-semibold leading-none tabular-nums lg:text-5xl"><AnimatedNumber value={num(value)} format={usd} /></p>}
            {allTimePct !== null && (
              <Badge variant="outline" className={cn("gap-1 rounded-full tabular-nums", trend === "down" ? "border-accent-error/40 text-accent-error" : trend === "up" ? "border-accent-warn/40 text-accent-warn" : "border-border text-ink-soft")}>
                {trend === "down" ? <TrendingDown /> : trend === "up" ? <TrendingUp /> : <Minus />}
                {t("dash.allTimeSuffix", { pct: formatPct(allTimePct, locale) })}
                <TipAnchor anchor="dashboard.performance.all-time-return" />
              </Badge>
            )}
          </div>
        </div>
        {/* Hand-written segmented control — uikit has no equivalent, so it carries its own focus ring. */}
        <div className="grid shrink-0 grid-cols-4 gap-0.5 rounded-lg border border-border bg-secondary p-1 lg:flex">
          {HISTORY_RANGES.map((r) => (
            <button
              key={r}
              type="button"
              aria-pressed={r === span.range}
              onClick={() => setSpan({ range: r, from: rangeFrom(r, Date.now()) })}
              className={cn(
                "rounded-md py-2 text-sm outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring lg:px-3 lg:py-1.5 lg:text-xs",
                r === span.range ? "bg-primary/15 font-semibold text-primary-ink" : "font-medium text-ink-soft hover:text-ink",
              )}
            >
              {t(RANGE_LABEL_KEYS[r])}
            </button>
          ))}
        </div>
      </div>
      <CardContent className="flex flex-col gap-3 px-0 lg:gap-5 lg:px-6 xl:flex-1">
        <div className="flex flex-wrap gap-x-4 gap-y-1.5 lg:order-2">
          <Legend dot="bg-chart-3" label={t("dash.fundPerformance")} />
          <Legend dot="bg-chart-2" label={t("dash.yourParticipation")} />
        </div>
        <PerfChart allocation={allocation} from={span.from} className="lg:order-1 xl:flex-1" />
      </CardContent>
    </StaggerItem>
  );
}

function Legend({ dot, label }: { dot: string; label: string }) {
  return (
    <span className="flex items-center gap-2 text-xs font-medium text-ink-soft">
      <span className={cn("size-2 rounded-full", dot)} />
      {label}
    </span>
  );
}
