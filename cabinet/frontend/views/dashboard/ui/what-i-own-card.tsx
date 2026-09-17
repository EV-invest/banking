"use client";

import { useT } from "@evinvest/i18n/react";
import { PieChart } from "lucide-react";

import { Button, Card, CardAction, CardContent, CardHeader, CardTitle, Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle, Progress, Skeleton } from "@evinvest/uikit";

import { cn } from "@/shared/lib/cn";
import { TipAnchor } from "@/shared/tips";
import { Link } from "@/shared/ui/cabinet-link";
import { Settled, StaggerItem } from "@/shared/ui/motion";
import { CARD_PAD, EMPTY_BOX } from "@/views/dashboard/lib/chrome";
import type { Allocation } from "@/views/dashboard/lib/holdings";

// The caller's holdings as shares of the whole, one bar per allocation (#245: per
// allocation, never "the fund").
export function WhatIOwnCard({ allocations, loading, className }: { allocations: Allocation[]; loading: boolean; className?: string }) {
  const t = useT();
  const total = allocations.reduce((s, a) => s + a.value, 0) || 1;
  return (
    <StaggerItem as={Card} className={cn("gap-3.5 py-4 lg:gap-4 lg:py-5", className)}>
      <CardHeader className={CARD_PAD}>
        <CardTitle className="flex items-center gap-1.5">
          {t("dash.investedWhatIOwn")}
          <TipAnchor anchor="dashboard.invested.allocation" />
        </CardTitle>
        <CardAction className="text-xs font-medium tabular-nums text-ink-soft">{t("dash.strategyCount", { n: allocations.length })}</CardAction>
      </CardHeader>
      <CardContent className={CARD_PAD}>
        <Settled loading={loading} skeleton={<Skeleton className="h-24 w-full" />}>
          {loading ? null : allocations.length === 0 ? (
            <Empty className={EMPTY_BOX}>
              <EmptyHeader>
                <EmptyMedia variant="icon">
                  <PieChart />
                </EmptyMedia>
                <EmptyTitle>{t("dash.nothingInvested")}</EmptyTitle>
                <EmptyDescription>{t("dash.nothingInvestedHint")}</EmptyDescription>
              </EmptyHeader>
              <EmptyContent>
                <Button asChild variant="outline">
                  <Link href="/invest">{t("dash.browseStrategies")}</Link>
                </Button>
              </EmptyContent>
            </Empty>
          ) : (
            <div className="flex flex-col gap-4">
              {allocations.map((a, i) => {
                const pct = Math.round((a.value / total) * 100);
                return (
                  <div key={`${a.name}-${i}`} className="flex flex-col gap-2">
                    <div className="flex items-center">
                      <span className="flex flex-1 items-center gap-2">
                        <span className={cn("size-2.5 rounded-full", a.accent.dot)} />
                        <span className="truncate text-sm font-medium text-ink-soft">{a.name}</span>
                      </span>
                      <span className="text-sm font-semibold tabular-nums text-ink">{pct}%</span>
                    </div>
                    <Progress value={pct} className={cn("h-1.5", a.accent.bar)} />
                  </div>
                );
              })}
            </div>
          )}
        </Settled>
      </CardContent>
    </StaggerItem>
  );
}
