"use client";

// The hero's plot: one allocation's valuation log over the chosen range, as the two series
// the legend above it names — the fund's return, and the caller's participation in USDT.
// This file owns the states of that surface (loading, empty, failed, drawn); the engine's
// lifecycle is `../lib/use-perf-chart`. The engine is downloaded on demand, so "loading"
// lasts until both the history and the engine are here — one skeleton, never an empty frame.

import { LineChart } from "lucide-react";
import { useLocale, useT } from "@evinvest/i18n/react";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle, Skeleton } from "@evinvest/uikit";
import { useMemo, useRef } from "react";

import { type NavSeries, toNavSeries } from "@/entities/fund/lib/nav-series";
import { fundNavHistoryResource } from "@/entities/fund/model/fund-history-resource";
import { cn } from "@/shared/lib/cn";
import { useResource } from "@/shared/lib/resource";
import { Settled } from "@/shared/ui/motion";
import { ReloadNotice } from "@/shared/ui/reload-notice";
import { ResourceError } from "@/shared/ui/resource-error";
import { EMPTY_BOX } from "@/views/dashboard/lib/chrome";
import { formatPct, formatUsdt } from "@/views/dashboard/lib/format";
import { perfView } from "@/views/dashboard/lib/perf-view";
import { type PerfFormat, usePerfChart, usePerfEngine } from "@/views/dashboard/lib/use-perf-chart";

// The plot's own height: tall enough for a curve to have a shape, and from `xl` whatever
// the hero has left after its header, so the card fills the side column's two rows.
const PLOT_BOX = "h-56 w-full lg:h-64 xl:h-full xl:min-h-56";

export interface PerfChartProps {
  /** The allocation whose log is drawn; `null` while the dashboard is still resolving which. */
  allocation: string | null;
  /** The window's lower bound in unix seconds (`rangeFrom`), or `undefined` for all-time. */
  from: number | undefined;
  className?: string;
}

export function PerfChart({ allocation, from, className }: PerfChartProps) {
  const t = useT();
  const locale = useLocale();
  const history = useResource(fundNavHistoryResource, allocation ?? "", from);
  const series = useMemo(() => toNavSeries(history.data), [history.data]);
  // Bound once per locale: the engine re-reads its formatters on identity. The participation
  // line is the caller's stake in USDT — the ledger unit, not the dashboard's summary "$".
  const format = useMemo<PerfFormat>(() => ({ performance: (pct) => formatPct(pct, locale), participation: (usdt) => formatUsdt(usdt, locale) }), [locale]);
  // Read (and so started) here, on mount, so the engine downloads alongside the history
  // request rather than after it. Only a plot with something to draw waits for it.
  const engine = usePerfEngine();
  const drawable = series.performance.length > 0;
  const view = perfView({ allocation, historyLoading: history.isLoading, historyFailed: history.data === undefined && history.error !== null, drawable, engine });
  const loading = view.kind === "skeleton";

  return (
    <Settled loading={loading} skeleton={<Skeleton className={PLOT_BOX} />} className={cn("flex flex-col gap-2", className)}>
      {loading ? null : view.kind === "history-error" ? (
        <ResourceError error={history.error} onRetry={history.refresh} retrying={history.isValidating} />
      ) : view.kind === "empty" ? (
        // No marks yet: the plot says so rather than drawing a line that traces back to
        // nothing. Its height is whatever the copy needs — a minimum belongs to a chart.
        <Empty className={EMPTY_BOX}>
          <EmptyHeader>
            <EmptyMedia variant="icon">
              <LineChart />
            </EmptyMedia>
            <EmptyTitle>{t("dash.noHistory", "No performance history yet")}</EmptyTitle>
            <EmptyDescription>{t("dash.noHistoryHint", "The fund curve and your participation appear here once there is activity to plot.")}</EmptyDescription>
          </EmptyHeader>
        </Empty>
      ) : view.kind === "reload" ? (
        // A chunk the bundler will not fetch again without a reload gets that action;
        // anything else the engine threw is reported like any failed read.
        <ReloadNotice className={cn(PLOT_BOX, "rounded-lg border border-border")} />
      ) : view.kind === "engine-error" ? (
        <ResourceError error={view.error} />
      ) : (
        <>
          <PerfPlot series={series} format={format} />
          {history.data?.truncated && <p className="text-xs text-ink-soft">{t("dash.historyTruncated", "Only the most recent marks in this range are shown; the fund curve starts from the oldest one shown, not from the start of the range.")}</p>}
        </>
      )}
    </Settled>
  );
}

// The host the engine mounts on. A component of its own so the host exists for exactly as
// long as there is something to draw: the engine is created on mount and torn down on
// unmount, and a host that came and went behind a single hook would leave it pointing at
// a detached element.
function PerfPlot({ series, format }: { series: NavSeries; format: PerfFormat }) {
  const host = useRef<HTMLDivElement>(null);
  usePerfChart(host, series, format);
  // The engine owns the host's children — nothing is ever drawn inside it.
  return <div ref={host} className={cn(PLOT_BOX, "overflow-hidden rounded-lg border border-border")} />;
}
