"use client";

// The hero's plot: one allocation's valuation log over the chosen range, as the two series
// the legend above it names — the fund's return, and the caller's participation in USDT.
// This file owns the four states of that surface (loading, empty, failed, drawn); the
// engine's lifecycle is `../lib/use-perf-chart`.

import { LineChart } from "lucide-react";
import { useLocale, useT } from "@evinvest/i18n/react";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle, Skeleton } from "@evinvest/uikit";
import { useMemo, useRef } from "react";

import { type NavSeries, toNavSeries } from "@/entities/fund/lib/nav-series";
import { fundNavHistoryResource } from "@/entities/fund/model/fund-history-resource";
import { cn } from "@/shared/lib/cn";
import { useResource } from "@/shared/lib/resource";
import { Settled } from "@/shared/ui/motion";
import { ResourceError } from "@/shared/ui/resource-error";
import { EMPTY_BOX } from "@/views/dashboard/lib/chrome";
import { formatPct, formatUsd } from "@/views/dashboard/lib/format";
import { type PerfFormat, usePerfChart } from "@/views/dashboard/lib/use-perf-chart";

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
  // Bound once per locale: the engine re-reads its formatters on identity.
  const format = useMemo<PerfFormat>(() => ({ performance: (pct) => formatPct(pct, locale), participation: (usdt) => formatUsd(usdt, locale) }), [locale]);
  const loading = allocation === null || history.isLoading;

  return (
    <Settled loading={loading} skeleton={<Skeleton className={PLOT_BOX} />} className={cn("flex flex-col gap-2", className)}>
      {loading ? null : history.data === undefined && history.error ? (
        <ResourceError error={history.error} onRetry={history.refresh} retrying={history.isValidating} />
      ) : series.performance.length === 0 ? (
        // No marks yet: the plot says so rather than drawing a line that traces back to
        // nothing. Its height is whatever the copy needs — a minimum belongs to a chart.
        <Empty className={EMPTY_BOX}>
          <EmptyHeader>
            <EmptyMedia variant="icon">
              <LineChart />
            </EmptyMedia>
            <EmptyTitle>{t("dash.noHistory")}</EmptyTitle>
            <EmptyDescription>{t("dash.noHistoryHint")}</EmptyDescription>
          </EmptyHeader>
        </Empty>
      ) : (
        <>
          <PerfPlot series={series} format={format} />
          {history.data?.truncated && <p className="text-xs text-ink-soft">{t("dash.historyTruncated")}</p>}
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
  return <div ref={host} className={cn(PLOT_BOX, "rounded-lg border border-border")} />;
}
