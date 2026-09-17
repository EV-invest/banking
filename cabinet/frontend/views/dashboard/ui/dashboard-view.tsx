"use client";

import { ArrowLeftRight, type LucideIcon, PieChart } from "lucide-react";
import type { Locale, Translate } from "@evinvest/i18n";
import { useLocale, useT } from "@evinvest/i18n/react";
import { Link } from "@/shared/ui/cabinet-link";
import { type CSSProperties, Fragment, useCallback } from "react";

import { Badge, Button, Card, CardAction, CardContent, CardHeader, CardTitle, Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle, Item, ItemActions, ItemContent, ItemDescription, ItemGroup, ItemMedia, ItemSeparator, ItemTitle, Progress, Skeleton } from "@evinvest/uikit";

import { allocationsResource, positionsResource } from "@/entities/fund/model/fund-resource";
import { RECENT_OPS, operationsResource } from "@/entities/operation/model/operation-resource";
import { walletResource } from "@/entities/wallet/model/wallet-resource";
import type { Operation } from "@/shared/contracts";
import { cn } from "@/shared/lib/cn";
import { useResource } from "@/shared/lib/resource";
import { SECTION_STAGGER, Settled, Stagger, StaggerItem } from "@/shared/ui/motion";
import { TipAnchor } from "@/shared/tips";
import { formatCount, STAT_STRIP, StatDivider, StatTile } from "@/shared/ui/stat-tile";
import { CARD_FROM_LG, CARD_PAD, EMPTY_BOX } from "@/views/dashboard/lib/chrome";
import { DASH_ADDRESS, formatSignedUsd, formatUsd, num, shortAddress, valence } from "@/views/dashboard/lib/format";
import { GetStartedSection } from "@/views/dashboard/ui/get-started-section";
import { PerfCard } from "@/views/dashboard/ui/perf-card";
import { amountTone, kindBadge, kindLabel, kindMeta, networkLabel, stateLabel } from "@/views/operations/lib/format";

// The card is a preview, not the record — `/operations` holds the full timeline. Asked
// of the hub rather than sliced client-side, so the six shown are the six most recent
// across all four kinds, not the newest six of whatever happened to be fetched. The count
// lives with the resource because the shell's warm-up has to ask for the same one.

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

type Accent = (typeof ACCENTS)[number];

// The portfolio dashboard (Figma `cabinet/home`). Bound to live wallet + fund-position
// data; a surface with nothing behind it yet is an honest empty state rather than a
// fabricated number.
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
  // a fund row should name the product, not the slug that keys it.
  const wallet = useResource(walletResource);
  const positions = useResource(positionsResource);
  const operations = useResource(operationsResource, RECENT_OPS);
  const catalogRead = useResource(allocationsResource);
  const catalog = catalogRead.data?.allocations ?? [];

  const balance = wallet.data?.balance;
  const pos = positions.data?.positions ?? [];
  const pnlSum = pos.reduce((s, p) => s + num(p.pnl), 0);
  const netContributed = pos.reduce((s, p) => s + num(p.cost_basis), 0);
  const allTimePct = netContributed > 0 ? (pnlSum / netContributed) * 100 : null;
  const walletLoading = wallet.isLoading;
  const posLoading = positions.isLoading;

  const titleOf = (service: string | undefined) => (service ? (catalog.find((a) => a.service === service)?.title ?? service) : t("dash.fundFallback"));
  // The chart is per allocation, never "the fund" (#245): the first one the caller holds,
  // or — for an account that holds nothing yet — the first open product, so a new investor
  // sees what the curve of the thing on offer looks like. `null` until both reads have
  // answered, so the plot shows a skeleton rather than an empty state that then fills.
  const allocation = posLoading || catalogRead.isLoading ? null : (pos[0]?.service ?? catalog[0]?.service ?? "");
  const allocations = pos.map((p, i) => ({ name: titleOf(p.service), value: num(p.value), accent: ACCENTS[i % ACCENTS.length]! }));
  const allocTotal = allocations.reduce((s, a) => s + a.value, 0) || 1;

  // The hub honours `limit`, so the slice is only a shape guarantee for the card.
  const ops = (operations.data?.operations ?? []).slice(0, RECENT_OPS).map((operation, i) => toOp(operation, i, titleOf, t, locale));

  // One DOM order, two layouts. Mobile stacks in reading order (hero → figures →
  // what I own → move money → activity); from `xl` the same children are placed
  // explicitly on a two-column grid so the desktop composition is unchanged. The
  // sidebar track is a fixed 360px with no matching step on the spacing scale, so it
  // rides in as a custom property instead of an arbitrary class.
  //
  // The grid is also the entrance: `Stagger` renders this same element, and each
  // section below is a `StaggerItem` rendered as the element it already was, so the
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
      <Stagger
        step={SECTION_STAGGER}
        className="grid grid-cols-1 gap-4 px-4 pb-6 pt-5 lg:gap-6 lg:px-8 lg:pb-7 lg:pt-6 xl:grid-cols-(--dash-columns) xl:items-start"
        style={{ "--dash-columns": "minmax(0, 1fr) 360px" } as CSSProperties}
      >
        {/* topbar — desktop only; on mobile the shell app bar plus the hero label carry the page */}
        <StaggerItem className="hidden items-center justify-between gap-4 lg:flex xl:col-span-2 xl:row-start-1">
          <div className="flex min-w-0 flex-col gap-1">
            <h1 className="text-2xl font-semibold leading-tight text-ink">{t("dash.portfolio")}</h1>
            <p className="text-sm text-ink-soft">{t("dash.portfolioSub")}</p>
          </div>
          {/* Shortcuts to the same two actions the Move money card offers, so they stay
              outline: one solid accent per screen, and that one belongs to the card that
              explains what it does. Two filled teal CTAs for the same destination read as
              loud rather than emphatic. */}
          <div className="flex shrink-0 gap-2.5">
            <Button asChild variant="outline">
              <Link href="/wallet/withdraw">{t("ui.withdraw")}</Link>
            </Button>
            <Button asChild variant="outline">
              <Link href="/wallet/deposit">{t("ui.deposit")}</Link>
            </Button>
          </div>
        </StaggerItem>

        <PerfCard value={balance?.total} loading={walletLoading} allTimePct={allTimePct} allocation={allocation} className="lg:order-1 xl:col-start-1 xl:row-span-2 xl:row-start-2" />

        {/* stat strip — a 2×2 card grid on mobile, one divided strip from `lg` */}
        <StaggerItem as={Card} className={cn(STAT_STRIP, CARD_FROM_LG, "lg:order-4 xl:col-span-2 xl:col-start-1 xl:row-start-4")}>
          <StatTile label={t("dash.unrealizedPnl")} value={walletLoading || posLoading ? null : pnlSum} format={signedUsd} tone={valence(pnlSum)} hint={t("dash.hintAcrossPositions")} tip="dashboard.stats.unrealized-pnl" />
          <StatDivider />
          <StatTile label={t("dash.available")} value={walletLoading ? null : num(balance?.available)} format={usd} hint={t("dash.hintAutoDeploysEod")} tip="dashboard.stats.available" />
          <StatDivider />
          <StatTile label={t("dash.activeStrategies")} value={posLoading ? null : pos.length} format={formatCount} hint={t("dash.hintFundPositions")} />
          <StatDivider />
          <StatTile label={t("dash.netContributed")} value={posLoading ? null : netContributed} format={usd} hint={t("dash.hintAtCostBasis")} tip="dashboard.stats.net-invested" />
        </StaggerItem>

        {/* Below `xl` the DOM order is the mobile order; `lg:order-*` restores the desktop
            sequence for the single-column band between `lg` and `xl`. */}
        <WhatIOwn allocations={allocations} total={allocTotal} loading={posLoading} className="lg:order-3 xl:col-start-2 xl:row-start-3" />
        <MoveMoney className="lg:order-2 xl:col-start-2 xl:row-start-2" />

        {/* operations */}
        <StaggerItem as={Card} className="gap-3 py-4 lg:order-5 lg:gap-4 lg:py-5 xl:col-span-2 xl:col-start-1 xl:row-start-5">
          <CardHeader className={CARD_PAD}>
            <CardTitle>{t("dash.recentOperations")}</CardTitle>
            <CardAction>
              <Button asChild variant="link" size="sm" className="px-0">
                <Link href="/operations">{t("ui.viewAll")}</Link>
              </Button>
            </CardAction>
          </CardHeader>
          <CardContent className={CARD_PAD}>
            {ops.length === 0 ? (
              <Empty className={EMPTY_BOX}>
                <EmptyHeader>
                  <EmptyMedia variant="icon">
                    <ArrowLeftRight />
                  </EmptyMedia>
                  <EmptyTitle>{t("ui.noOperations")}</EmptyTitle>
                  <EmptyDescription>{t("dash.noOperationsHint")}</EmptyDescription>
                </EmptyHeader>
                <EmptyContent>
                  {/* Same destination as the Move money card's filled Deposit, which is
                      already on screen — so this one stays outline. */}
                  <Button asChild variant="outline">
                    <Link href="/wallet/deposit">{t("ui.addFunds")}</Link>
                  </Button>
                </EmptyContent>
              </Empty>
            ) : (
              <ItemGroup>
                {ops.map((op, i) => (
                  <Fragment key={op.id}>
                    {i > 0 && <ItemSeparator />}
                    <Item size="sm" className="px-0 py-3 lg:py-4">
                      <ItemMedia>
                        {/* Decorative: the row title names the kind in words right beside it. */}
                        <Badge className={cn("font-semibold", op.tagClass)}>{op.icon ? <op.icon aria-hidden /> : op.tag}</Badge>
                      </ItemMedia>
                      <ItemContent className="min-w-0 gap-0.5">
                        <ItemTitle className="block w-auto truncate font-semibold">{op.title}</ItemTitle>
                        <ItemDescription className="line-clamp-1 text-xs">{op.sub}</ItemDescription>
                      </ItemContent>
                      <ItemActions className={cn("shrink-0 text-sm font-semibold tabular-nums", op.amountClass)}>{op.amount}</ItemActions>
                    </Item>
                  </Fragment>
                ))}
              </ItemGroup>
            )}
          </CardContent>
        </StaggerItem>
      </Stagger>
    </>
  );
}

// Two actions and nothing else: the "auto-deploy idle cash" switch that used to sit
// below them had no feature behind it (#397) and is gone until one exists.
function MoveMoney({ className }: { className?: string }) {
  const t = useT();
  return (
    <StaggerItem as={Card} className={cn("gap-3.5 py-4 lg:gap-4 lg:py-5", className)}>
      <CardHeader className={CARD_PAD}>
        <CardTitle>{t("dash.moveMoney")}</CardTitle>
      </CardHeader>
      <CardContent className={cn("flex gap-2.5", CARD_PAD)}>
        <Button asChild className="flex-1">
          <Link href="/wallet/deposit">{t("ui.deposit")}</Link>
        </Button>
        <Button asChild variant="outline" className="flex-1">
          <Link href="/wallet/withdraw">{t("ui.withdraw")}</Link>
        </Button>
      </CardContent>
    </StaggerItem>
  );
}

function WhatIOwn({ allocations, total, loading, className }: { allocations: { name: string; value: number; accent: Accent }[]; total: number; loading: boolean; className?: string }) {
  const t = useT();
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

interface Op {
  id: string;
  /** The kind's mark; `null` for a kind this build has no mark for, which falls back to
   *  {@link tag}. Same rule as the operations timeline — the vocabulary is shared. */
  icon: LucideIcon | null;
  /** The text a mark-less kind wears instead. */
  tag: string;
  tagClass: string;
  title: string;
  sub: string;
  amount: string;
  amountClass: string;
}

// One timeline row rendered in the dashboard's summary-money policy (`formatUsd`, to the
// cent with a currency symbol) rather than the ledger policy the Operations page uses —
// same data, different unit of measure for the surface it sits on. The badge, tone and
// sign vocabulary is shared with `/operations` so a row reads identically in both places.
function toOp(operation: Operation, index: number, titleOf: (service: string | undefined) => string, t: Translate, locale: Locale): Op {
  const meta = kindMeta(operation.kind);
  const sign = meta.direction === "in" ? "+" : meta.direction === "out" ? "\u2212" : "";
  return {
    id: `${operation.kind ?? ""}-${operation.id ?? ""}-${index}`,
    icon: meta.icon,
    tag: kindBadge(operation.kind),
    tagClass: meta.tone,
    title: opTitle(operation, titleOf, t),
    sub: opSub(operation, t),
    // A queued redemption is not yet priced, so it shows the units it reserved — a
    // formatted zero would claim the user was paid nothing.
    amount: operation.amount ? `${sign}${formatUsd(operation.amount, locale)}` : t("dash.unitsAmount", { n: Number(operation.units ?? 0), units: operation.units ?? "0" }),
    amountClass: operation.amount ? amountTone(meta.direction) : "text-ink-soft",
  };
}

function opTitle(operation: Operation, titleOf: (service: string | undefined) => string, t: Translate): string {
  if (operation.kind === "subscription") return t("dash.op.subscribed", { fund: titleOf(operation.service) });
  if (operation.kind === "redemption") return t("dash.op.redeemed", { fund: titleOf(operation.service) });
  if (operation.kind === "fee") return t("ops.op.feeCharged", { fund: titleOf(operation.service) });
  if (operation.kind === "withdrawal") return t("dash.op.withdrawal", { network: networkLabel(operation.network) });
  if (operation.kind === "deposit") return t("dash.op.deposit", { network: networkLabel(operation.network) });
  return kindLabel(operation.kind, t);
}

function opSub(operation: Operation, t: Translate): string {
  // The lifecycle state reaches the reader through the same vocabulary the operations
  // timeline uses \u2014 it used to be the bare wire identifier, English by construction.
  const state = stateLabel(operation.state, t);
  if (operation.kind === "withdrawal") return t("dash.op.sub", { ref: shortAddress(operation.address, DASH_ADDRESS), state });
  if (operation.kind === "deposit") return t("dash.op.sub", { ref: shortAddress(operation.tx_ref, DASH_ADDRESS), state });
  return state;
}
