"use client";

import { useLocale, useT } from "@evinvest/i18n/react";
import { type CSSProperties, useCallback } from "react";

import { Button, Card } from "@evinvest/uikit";

import { allocationsResource, positionsResource } from "@/entities/fund/model/fund-resource";
import { RECENT_OPS, operationsResource } from "@/entities/operation/model/operation-resource";
import { walletResource } from "@/entities/wallet/model/wallet-resource";
import { cn } from "@/shared/lib/cn";
import { useResource } from "@/shared/lib/resource";
import { Link } from "@/shared/ui/cabinet-link";
import { StaggerItem } from "@/shared/ui/motion";
import { PageFrame } from "@/shared/ui/page-frame";
import { formatCount, STAT_STRIP, StatDivider, StatTile } from "@/shared/ui/stat-tile";
import { CARD_FROM_LG } from "@/views/dashboard/lib/chrome";
import { formatSignedUsd, formatUsd, num, valence } from "@/views/dashboard/lib/format";
import { summariseHoldings, toAllocations } from "@/views/dashboard/lib/holdings";
import { toOp } from "@/views/dashboard/lib/recent-ops";
import { GetStartedSection } from "@/views/dashboard/ui/get-started-section";
import { MoveMoneyCard } from "@/views/dashboard/ui/move-money-card";
import { PerfCard } from "@/views/dashboard/ui/perf-card";
import { RecentOperationsCard } from "@/views/dashboard/ui/recent-operations-card";
import { WhatIOwnCard } from "@/views/dashboard/ui/what-i-own-card";

// The portfolio dashboard (Figma `cabinet/home`). Bound to live wallet + fund-position
// data; a surface with nothing behind it yet is an honest empty state rather than a
// fabricated number. This file only composes: the figures are `lib/holdings`, the rows
// `lib/recent-ops`, and each card its own file.
export function DashboardView() {
  const t = useT();
  const locale = useLocale();
  // Bound once per locale, not inline: AnimatedNumber restarts its count whenever the
  // identity of `format` changes, and a fresh closure every render would restart it every
  // render.
  const usd = useCallback((n: number) => formatUsd(n, locale), [locale]);
  const signedUsd = useCallback((n: number) => formatSignedUsd(n, locale), [locale]);
  // All four reads are shared with other screens and cached, so a return to Home paints the
  // balance, the holdings and the timeline on the first frame — the skeletons below are for
  // the cold first load only. The catalog is the same registry the rail lists products from:
  // a fund row should name the product, not the slug that keys it. The recent-operations
  // count is asked of the hub rather than sliced client-side, so the six shown are the six
  // most recent across all four kinds — it lives with the resource because the shell's
  // warm-up has to ask for the same one.
  const wallet = useResource(walletResource);
  const positions = useResource(positionsResource);
  const operations = useResource(operationsResource, RECENT_OPS);
  const catalogRead = useResource(allocationsResource);
  const catalog = catalogRead.data?.allocations ?? [];

  const balance = wallet.data?.balance;
  const pos = positions.data?.positions ?? [];
  const { pnl, netContributed, allTimePct } = summariseHoldings(pos);
  const walletLoading = wallet.isLoading;
  const posLoading = positions.isLoading;

  const titleOf = (service: string | undefined) => (service ? (catalog.find((a) => a.service === service)?.title ?? service) : t("dash.fundFallback"));
  // The chart is per allocation, never "the fund" (#245): the first one the caller holds,
  // or — for an account that holds nothing yet — the first open product, so a new investor
  // sees what the curve of the thing on offer looks like. `null` until both reads have
  // answered, so the plot shows a skeleton rather than an empty state that then fills.
  const allocation = posLoading || catalogRead.isLoading ? null : (pos[0]?.service ?? catalog[0]?.service ?? "");
  // The hub honours `limit`, so the slice is only a shape guarantee for the card.
  const ops = (operations.data?.operations ?? []).slice(0, RECENT_OPS).map((operation, i) => toOp(operation, i, titleOf, t, locale));

  // One DOM order, two layouts. Mobile stacks in reading order (hero → figures →
  // what I own → move money → activity); from `xl` the same children are placed
  // explicitly on a two-column grid so the desktop composition is unchanged. The
  // sidebar track is a fixed 360px with no matching step on the spacing scale, so it
  // rides in as a custom property instead of an arbitrary class.
  //
  // The grid is also the entrance: the frame's `Stagger` renders this same element, and
  // each section below is a `StaggerItem` rendered as the element it already was, so the
  // placement classes stay on the grid items that carry them. The sequence follows
  // DOM order — which on mobile is reading order, and on desktop is close enough
  // that no section arrives before the one above it.
  return (
    <>
      {/* Above the grid rather than inside it: from `xl` the grid places its children on
          explicitly numbered rows, and an onboarding block that renumbered them would move
          the whole desktop composition for one temporary state. Above is also the point: for
          an account with a step still to do, the path takes the top slot and the hero — a
          zero over an empty plot — reads second. Once the path is done it is one quiet line. */}
      <GetStartedSection className="mx-4 mt-5 lg:mx-8 lg:mt-6" />
      <PageFrame
        title={t("dash.portfolio")}
        description={t("dash.portfolioSub")}
        // Shortcuts to the same two actions the Move money card offers, so they stay
        // outline: one solid accent per screen, and that one belongs to the card that
        // explains what it does. Two filled teal CTAs for the same destination read as
        // loud rather than emphatic.
        actions={
          <>
            <Button asChild variant="outline">
              <Link href="/wallet/withdraw">{t("ui.withdraw")}</Link>
            </Button>
            <Button asChild variant="outline">
              <Link href="/wallet/deposit">{t("ui.deposit")}</Link>
            </Button>
          </>
        }
        // Desktop only: on mobile the shell app bar plus the hero label carry the page.
        headingClassName="hidden lg:flex xl:col-span-2 xl:row-start-1"
        className="grid grid-cols-1 xl:grid-cols-(--dash-columns) xl:items-start"
        style={{ "--dash-columns": "minmax(0, 1fr) 360px" } as CSSProperties}
      >
        <PerfCard value={balance?.total} loading={walletLoading} allTimePct={allTimePct} allocation={allocation} className="lg:order-1 xl:col-start-1 xl:row-span-2 xl:row-start-2" />

        {/* stat strip — a 2×2 card grid on mobile, one divided strip from `lg` */}
        <StaggerItem as={Card} className={cn(STAT_STRIP, CARD_FROM_LG, "lg:order-4 xl:col-span-2 xl:col-start-1 xl:row-start-4")}>
          <StatTile label={t("dash.unrealizedPnl")} value={walletLoading || posLoading ? null : pnl} format={signedUsd} tone={valence(pnl)} hint={t("dash.hintAcrossPositions")} tip="dashboard.stats.unrealized-pnl" />
          <StatDivider />
          <StatTile label={t("dash.available")} value={walletLoading ? null : num(balance?.available)} format={usd} hint={t("dash.hintAutoDeploysEod")} tip="dashboard.stats.available" />
          <StatDivider />
          <StatTile label={t("dash.activeStrategies")} value={posLoading ? null : pos.length} format={formatCount} hint={t("dash.hintFundPositions")} />
          <StatDivider />
          <StatTile label={t("dash.netContributed")} value={posLoading ? null : netContributed} format={usd} hint={t("dash.hintAtCostBasis")} tip="dashboard.stats.net-invested" />
        </StaggerItem>

        {/* Below `xl` the DOM order is the mobile order; `lg:order-*` restores the desktop
            sequence for the single-column band between `lg` and `xl`. */}
        <WhatIOwnCard allocations={toAllocations(pos, titleOf)} loading={posLoading} className="lg:order-3 xl:col-start-2 xl:row-start-3" />
        <MoveMoneyCard className="lg:order-2 xl:col-start-2 xl:row-start-2" />
        <RecentOperationsCard ops={ops} className="lg:order-5 xl:col-span-2 xl:col-start-1 xl:row-start-5" />
      </PageFrame>
    </>
  );
}
