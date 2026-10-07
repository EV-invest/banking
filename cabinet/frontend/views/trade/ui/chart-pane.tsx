"use client";

// The price chart: candles from the hub's history, extended live from the tape. The
// engine is mounted on the uikit's `TerminalChart` host by `useCandleChart`; this file
// owns only the resolution switch and the three states drawn over an empty canvas.

import { useMemo, useRef, useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { TerminalChart, TerminalPane, TerminalPaneBody, TerminalPaneHeader, ToggleGroup, ToggleGroupItem } from "@evinvest/uikit";

import { RESOLUTIONS, isResolution } from "@/entities/book/lib/vocabulary";
import type { CandleResolution } from "@/shared/contracts/book";
import { errorMessage } from "@/shared/lib/api-client";
import { cn } from "@/shared/lib/cn";
import { ReloadNotice } from "@/shared/ui/reload-notice";
import { restChartFeed } from "@/views/trade/lib/chart-feed";
import { chartOverlay } from "@/views/trade/lib/chart-overlay";
import { useCandleChart } from "@/views/trade/lib/use-candle-chart";

const DEFAULT_RESOLUTION: CandleResolution = "1h";

export function ChartPane({ service }: { service: string }) {
  const t = useT();
  const [resolution, setResolution] = useState<CandleResolution>(DEFAULT_RESOLUTION);
  // One feed per product for the life of the pane: the hook re-subscribes on identity.
  const feed = useMemo(() => restChartFeed(service), [service]);
  const host = useRef<HTMLDivElement>(null);
  const state = useCandleChart(host, feed, resolution);
  const overlay = chartOverlay(state);

  return (
    <TerminalPane area="chart">
      <TerminalPaneHeader className="justify-between">
        <span>{t("trade.chart.title", "Chart")}</span>
        <ToggleGroup type="single" size="sm" value={resolution} onValueChange={(v) => isResolution(v) && setResolution(v)} aria-label={t("trade.chart.resolution", "Candle size")}>
          {RESOLUTIONS.map((r) => (
            <ToggleGroupItem key={r} value={r} className="px-2 font-mono-tech text-xs">
              {r}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
      </TerminalPaneHeader>
      <TerminalPaneBody className="relative">
        <TerminalChart ref={host} className={cn(state.kind !== "ready" && "opacity-40")} />
        {overlay.kind === "reload" ? (
          // The engine's chunk never arrived, and the bundler will not fetch it again
          // without a reload — "please try again" would be an instruction nothing obeys.
          <ReloadNotice className="absolute inset-0" />
        ) : overlay.kind !== "none" && (
          // Drawn beside the host, never inside it: the engine owns the host's children.
          <p className={cn("pointer-events-none absolute inset-0 flex items-center justify-center px-6 text-center text-xs", overlay.kind === "error" ? "text-accent-error" : "text-ink-soft")}>
            {overlay.kind === "loading" ? t("ui.loading", "Loading…") : overlay.kind === "empty" ? t("trade.chart.empty", "No trades yet — the chart draws from the first fill.") : errorMessage(overlay.error, t)}
          </p>
        )}
      </TerminalPaneBody>
    </TerminalPane>
  );
}
