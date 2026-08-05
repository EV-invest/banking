"use client";

import { ArrowLeftRight, LineChart, PieChart, TrendingDown, TrendingUp } from "lucide-react";
import Link from "next/link";
import { type CSSProperties, Fragment, useEffect, useState } from "react";

import { Badge, Button, Card, CardAction, CardContent, CardHeader, CardTitle, Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle, Item, ItemActions, ItemContent, ItemDescription, ItemGroup, ItemMedia, ItemSeparator, ItemTitle, Progress, Separator, Skeleton, Switch } from "@evinvest/uikit";

import { fetchPositions, fetchRedemptions } from "@/entities/fund/api/fund-client";
import { fetchWallet, fetchWithdrawals } from "@/entities/wallet/api/wallet-client";
import type { Position, Redemption, Wallet, Withdrawal } from "@/shared/contracts";
import { cn } from "@/shared/lib/cn";
import { TipAnchor, type TipKey } from "@/shared/tips";
import { formatMoney, formatPct, formatSignedMoney, num, shortAddress } from "@/views/dashboard/lib/format";

const RANGES = ["1M", "6M", "1Y", "All"] as const;

// Allocation slices cycle the brand accent tiers. The bar names its tier twice because
// Progress paints track and indicator from `--primary`, and the child selector is the only
// way to reach the indicator without forking the component.
const ACCENTS = [
  { dot: "bg-main-accent-t1", bar: "bg-main-accent-t1/20 *:bg-main-accent-t1" },
  { dot: "bg-main-accent-t2", bar: "bg-main-accent-t2/20 *:bg-main-accent-t2" },
  { dot: "bg-main-accent-t3", bar: "bg-main-accent-t3/20 *:bg-main-accent-t3" },
  { dot: "bg-main-accent-t4", bar: "bg-main-accent-t4/20 *:bg-main-accent-t4" },
] as const;

type Accent = (typeof ACCENTS)[number];

// Cards inset 16px on mobile (Figma `cabinet/mobile/home`), the uikit 24px from `lg`.
const CARD_PAD = "px-4 lg:px-6";

// Mobile reads the hero and the stat strip as page content rather than as cards: those two
// surfaces sit flat on the background and only take their Card chrome from `lg`.
const CARD_FROM_LG = "rounded-none border-0 bg-transparent py-0 shadow-none lg:rounded-xl lg:border lg:bg-card lg:py-5 lg:shadow-sm";

// uikit's Empty draws a dashed frame but leaves the border width to the caller, and doubles
// its padding at `md`; these sit inside cards, not on a page of their own.
const EMPTY_BOX = "border md:p-6";

// The portfolio dashboard (Figma `cabinet/home`). Bound to live wallet + fund-position
// data; figures with no backing series yet (the performance chart) are honest empty states
// rather than fabricated numbers.
export function DashboardView() {
  const [wallet, setWallet] = useState<Wallet | null | undefined>(undefined);
  const [positions, setPositions] = useState<Position[] | undefined>(undefined);
  const [withdrawals, setWithdrawals] = useState<Withdrawal[]>([]);
  const [redemptions, setRedemptions] = useState<Redemption[]>([]);

  useEffect(() => {
    fetchWallet()
      .then(setWallet)
      .catch(() => setWallet(null));
    fetchPositions()
      .then((l) => setPositions(l.positions ?? []))
      .catch(() => setPositions([]));
    fetchWithdrawals()
      .then((l) => setWithdrawals(l.withdrawals ?? []))
      .catch(() => undefined);
    fetchRedemptions()
      .then((l) => setRedemptions(l.redemptions ?? []))
      .catch(() => undefined);
  }, []);

  const balance = wallet?.balance;
  const pos = positions ?? [];
  const pnlSum = pos.reduce((s, p) => s + num(p.pnl), 0);
  const netContributed = pos.reduce((s, p) => s + num(p.cost_basis), 0);
  const allTimePct = netContributed > 0 ? (pnlSum / netContributed) * 100 : null;
  const walletLoading = wallet === undefined;
  const posLoading = positions === undefined;

  const allocations = pos.map((p, i) => ({ name: p.service ?? "Fund", value: num(p.value), accent: ACCENTS[i % ACCENTS.length]! }));
  const allocTotal = allocations.reduce((s, a) => s + a.value, 0) || 1;

  const ops = buildOps(redemptions, withdrawals);

  return (
    // One DOM order, two layouts. Mobile stacks in reading order (hero → figures →
    // what I own → move money → activity); from `xl` the same children are placed
    // explicitly on a two-column grid so the desktop composition is unchanged. The
    // sidebar track is a fixed 360px with no matching step on the spacing scale, so it
    // rides in as a custom property instead of an arbitrary class.
    <div
      className="grid grid-cols-1 gap-4 px-4 pb-6 pt-5 lg:gap-6 lg:px-8 lg:pb-7 lg:pt-6 xl:grid-cols-(--dash-columns) xl:items-start"
      style={{ "--dash-columns": "minmax(0, 1fr) 360px" } as CSSProperties}
    >
      {/* topbar — desktop only; on mobile the shell app bar plus the hero label carry the page */}
      <div className="hidden items-center justify-between gap-4 lg:flex xl:col-span-2 xl:row-start-1">
        <div className="flex min-w-0 flex-col gap-1">
          <h1 className="font-sans text-2xl font-semibold leading-tight text-foreground">Portfolio</h1>
          <p className="text-sm text-muted-foreground">All-time performance and your participation</p>
        </div>
        <div className="flex shrink-0 gap-2.5">
          <Button asChild variant="outline">
            <Link href="/wallet/withdraw">Withdraw</Link>
          </Button>
          <Button asChild>
            <Link href="/wallet/deposit">Deposit</Link>
          </Button>
        </div>
      </div>

      <PerfCard value={balance?.total} loading={walletLoading} allTimePct={allTimePct} className="lg:order-1 xl:col-start-1 xl:row-span-2 xl:row-start-2" />

      {/* stat strip — a 2×2 card grid on mobile, one divided strip from `lg` */}
      <Card className={cn("grid grid-cols-2 gap-3 lg:flex lg:flex-row lg:flex-wrap lg:items-stretch lg:gap-x-7 lg:gap-y-4 lg:px-6", CARD_FROM_LG, "lg:order-4 xl:col-span-2 xl:col-start-1 xl:row-start-4")}>
        <Stat label="Unrealized P&L" value={walletLoading || posLoading ? null : formatSignedMoney(pnlSum)} tone={pnlSum < 0 ? "loss" : "gain"} hint="across all positions" tip="dashboard.stats.unrealized-pnl" />
        <Separator orientation="vertical" className="hidden self-stretch lg:block" />
        <Stat label="Available" value={walletLoading ? null : formatMoney(balance?.available)} hint="auto-deploys at EOD" tip="dashboard.stats.available" />
        <Separator orientation="vertical" className="hidden self-stretch lg:block" />
        <Stat label="Active strategies" value={posLoading ? null : String(pos.length)} hint="fund positions" />
        <Separator orientation="vertical" className="hidden self-stretch lg:block" />
        <Stat label="Net contributed" value={posLoading ? null : formatMoney(netContributed)} hint="at cost basis" tip="dashboard.stats.net-invested" />
      </Card>

      {/* Below `xl` the DOM order is the mobile order; `lg:order-*` restores the desktop
          sequence for the single-column band between `lg` and `xl`. */}
      <WhatIOwn allocations={allocations} total={allocTotal} loading={posLoading} className="lg:order-3 xl:col-start-2 xl:row-start-3" />
      <MoveMoney className="lg:order-2 xl:col-start-2 xl:row-start-2" />

      {/* operations */}
      <Card className="gap-3 py-4 lg:order-5 lg:gap-4 lg:py-5 xl:col-span-2 xl:col-start-1 xl:row-start-5">
        <CardHeader className={CARD_PAD}>
          <CardTitle>Recent operations</CardTitle>
          <CardAction>
            <Button asChild variant="link" size="sm" className="px-0">
              <Link href="/operations">View all</Link>
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
                <EmptyTitle>No operations yet</EmptyTitle>
                <EmptyDescription>Subscriptions, redemptions and withdrawals land here as they settle.</EmptyDescription>
              </EmptyHeader>
              <EmptyContent>
                <Button asChild>
                  <Link href="/wallet/deposit">Add funds</Link>
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
                      <Badge className={cn("font-semibold", op.tagClass)}>{op.tag}</Badge>
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
      </Card>
    </div>
  );
}

// Mobile (Figma `cabinet/mobile/home`) reads the hero as page content, not as a card: the
// value sits flat on the background, the range switch spans the width below it, and only the
// plot is boxed — with its legend above. From `lg` the whole block is the desktop card again.
function PerfCard({ value, loading, allTimePct, className }: { value: string | undefined; loading: boolean; allTimePct: number | null; className?: string }) {
  const [range, setRange] = useState<(typeof RANGES)[number]>("All");
  const down = (allTimePct ?? 0) < 0;
  return (
    // From `xl` the hero spans both rows of the side column, so it has to fill that
    // area — otherwise the plot area keeps its natural height and leaves a gap under
    // the card whenever the side column is the taller of the two.
    <Card className={cn("flex-1 gap-4 lg:gap-5 xl:h-full", CARD_FROM_LG, className)}>
      <div className="flex flex-col gap-3.5 lg:flex-row lg:items-start lg:justify-between lg:gap-4 lg:px-6">
        <div className="flex min-w-0 flex-col gap-2">
          <p className="flex items-center gap-1.5 text-xs font-medium uppercase tracking-wider text-main-accent-t1">
            Portfolio value
            <TipAnchor anchor="dashboard.performance.portfolio-value" />
          </p>
          <div className="flex flex-col items-start gap-2.5 lg:flex-row lg:items-center lg:gap-3.5">
            {loading ? <Skeleton className="h-10 w-40 lg:h-12 lg:w-48" /> : <p className="text-4xl font-semibold leading-none tabular-nums lg:text-5xl">{formatMoney(value)}</p>}
            {allTimePct !== null && (
              <Badge variant="outline" className={cn("gap-1 rounded-full tabular-nums", down ? "border-destructive/40 text-destructive" : "border-main-accent-t3/40 text-main-accent-t3")}>
                {down ? <TrendingDown /> : <TrendingUp />}
                {formatPct(allTimePct)} all-time
                <TipAnchor anchor="dashboard.performance.all-time-return" />
              </Badge>
            )}
          </div>
        </div>
        {/* Hand-written segmented control — uikit has no equivalent, so it carries its own focus ring. */}
        <div className="grid shrink-0 grid-cols-4 gap-0.5 rounded-lg border border-border bg-main-surface p-1 lg:flex">
          {RANGES.map((r) => (
            <button
              key={r}
              type="button"
              aria-pressed={r === range}
              onClick={() => setRange(r)}
              className={cn(
                "rounded-md py-2 text-sm outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring/50 lg:px-3 lg:py-1.5 lg:text-xs",
                r === range ? "bg-main-accent-t1/15 font-semibold text-main-accent-t1" : "font-medium text-muted-foreground hover:text-foreground",
              )}
            >
              {r}
            </button>
          ))}
        </div>
      </div>
      <CardContent className="flex flex-col gap-3 px-0 lg:gap-5 lg:px-6 xl:flex-1">
        <div className="flex flex-wrap gap-x-4 gap-y-1.5 lg:order-2">
          <Legend dot="bg-main-accent-t3" label="Fund performance" />
          <Legend dot="bg-main-accent-t2" label="Your participation" />
        </div>
        {/* No performance series exists in `shared/contracts` yet, so the plot area says so
            rather than drawing a line that traces back to nothing. */}
        <Empty className={cn(EMPTY_BOX, "min-h-40 lg:order-1 lg:min-h-56")}>
          <EmptyHeader>
            <EmptyMedia variant="icon">
              <LineChart />
            </EmptyMedia>
            <EmptyTitle>No performance history yet</EmptyTitle>
            <EmptyDescription>The fund curve and your participation appear here once there is activity to plot.</EmptyDescription>
          </EmptyHeader>
        </Empty>
      </CardContent>
    </Card>
  );
}

function Legend({ dot, label }: { dot: string; label: string }) {
  return (
    <span className="flex items-center gap-2 text-xs font-medium text-muted-foreground">
      <span className={cn("size-2 rounded-full", dot)} />
      {label}
    </span>
  );
}

function MoveMoney({ className }: { className?: string }) {
  const [auto, setAuto] = useState(true);
  return (
    <Card className={cn("gap-3.5 py-4 lg:gap-4 lg:py-5", className)}>
      <CardHeader className={CARD_PAD}>
        <CardTitle>Move money</CardTitle>
      </CardHeader>
      <CardContent className={cn("flex flex-col gap-3.5 lg:gap-4", CARD_PAD)}>
        <div className="flex gap-2.5">
          <Button asChild className="flex-1">
            <Link href="/wallet/deposit">Deposit</Link>
          </Button>
          <Button asChild variant="outline" className="flex-1">
            <Link href="/wallet/withdraw">Withdraw</Link>
          </Button>
        </div>
        <Item variant="outline" size="sm" className="rounded-lg bg-main-surface">
          <ItemContent className="gap-0.5">
            <ItemTitle id="auto-deploy-label" className="gap-1.5">
              Auto-deploy idle cash
              <TipAnchor anchor="dashboard.move-money.auto-deploy" />
            </ItemTitle>
            <ItemDescription className="text-xs">Available balance commits at end of day</ItemDescription>
          </ItemContent>
          <ItemActions>
            <Switch checked={auto} onCheckedChange={setAuto} aria-labelledby="auto-deploy-label" />
          </ItemActions>
        </Item>
      </CardContent>
    </Card>
  );
}

function WhatIOwn({ allocations, total, loading, className }: { allocations: { name: string; value: number; accent: Accent }[]; total: number; loading: boolean; className?: string }) {
  return (
    <Card className={cn("gap-3.5 py-4 lg:gap-4 lg:py-5", className)}>
      <CardHeader className={CARD_PAD}>
        <CardTitle className="flex items-center gap-1.5">
          Invested · what I own
          <TipAnchor anchor="dashboard.invested.allocation" />
        </CardTitle>
        <CardAction className="text-xs font-medium tabular-nums text-muted-foreground">
          {allocations.length} {allocations.length === 1 ? "strategy" : "strategies"}
        </CardAction>
      </CardHeader>
      <CardContent className={CARD_PAD}>
        {loading ? (
          <Skeleton className="h-24 w-full" />
        ) : allocations.length === 0 ? (
          <Empty className={EMPTY_BOX}>
            <EmptyHeader>
              <EmptyMedia variant="icon">
                <PieChart />
              </EmptyMedia>
              <EmptyTitle>Nothing invested yet</EmptyTitle>
              <EmptyDescription>Subscribe to a strategy and the split of what you own shows up here.</EmptyDescription>
            </EmptyHeader>
            <EmptyContent>
              <Button asChild variant="outline">
                <Link href="/invest">Browse strategies</Link>
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
                      <span className="truncate text-sm font-medium text-muted-foreground">{a.name}</span>
                    </span>
                    <span className="text-sm font-semibold tabular-nums text-foreground">{pct}%</span>
                  </div>
                  <Progress value={pct} className={cn("h-1.5", a.accent.bar)} />
                </div>
              );
            })}
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function Stat({ label, value, tone, hint, tip }: { label: string; value: string | null; tone?: "gain" | "loss"; hint: string; tip?: TipKey }) {
  const valueClass = tone === "gain" ? "text-main-accent-t2" : tone === "loss" ? "text-destructive" : "text-foreground";
  const hintClass = tone === "gain" ? "text-main-accent-t2/80" : tone === "loss" ? "text-destructive/80" : "text-muted-foreground";
  return (
    // Its own tile on mobile, a cell of the shared strip from `lg`.
    <Card className="min-w-0 flex-1 gap-1 px-3.5 py-3 lg:min-w-30 lg:gap-1.5 lg:rounded-none lg:border-0 lg:bg-transparent lg:p-0 lg:shadow-none">
      <div className="flex items-center gap-1.5">
        <p className="truncate text-xs font-medium text-muted-foreground">{label}</p>
        {tip && <TipAnchor anchor={tip} />}
      </div>
      {value === null ? <Skeleton className="h-6 w-20" /> : <p className={cn("truncate text-xl font-semibold tabular-nums lg:text-2xl", valueClass)}>{value}</p>}
      <p className={cn("truncate text-xs font-medium", hintClass)}>{hint}</p>
    </Card>
  );
}

interface Op {
  id: string;
  tag: string;
  tagClass: string;
  title: string;
  sub: string;
  amount: string;
  amountClass: string;
}

// A modest unified feed from what the BFF exposes today (redemptions + withdrawals).
// Deposits/subscriptions get their own events once the hub surfaces an operations stream.
function buildOps(redemptions: Redemption[], withdrawals: Withdrawal[]): Op[] {
  const fromRedemptions: Op[] = redemptions.map((r, i) => ({
    id: r.id ?? `r-${i}`,
    tag: "REDEEM",
    tagClass: "bg-main-accent-t1/15 text-main-accent-t1",
    title: `${r.service ?? "Fund"} — redeemed`,
    sub: r.state ?? "queued",
    amount: r.cash ? `+${formatMoney(r.cash)}` : `${r.units ?? "0"} units`,
    amountClass: r.cash ? "text-main-accent-t2" : "text-main-mist",
  }));
  const fromWithdrawals: Op[] = withdrawals.map((w, i) => ({
    id: w.id ?? `w-${i}`,
    tag: "OUT",
    tagClass: "bg-destructive/15 text-destructive",
    title: `Withdrawal · ${(w.network ?? "").toUpperCase()}`,
    sub: `${shortAddress(w.address)} · ${w.state ?? ""}`,
    amount: `−${formatMoney(w.net_amount ?? w.amount)}`,
    amountClass: "text-destructive",
  }));
  return [...fromRedemptions, ...fromWithdrawals].slice(0, 6);
}
