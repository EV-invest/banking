"use client";

import { TrendingDown, TrendingUp } from "lucide-react";
import Link from "next/link";
import { useEffect, useState } from "react";

import { Skeleton } from "@evinvest/uikit";

import { fetchPositions, fetchRedemptions } from "@/entities/fund/api/fund-client";
import { fetchWallet, fetchWithdrawals } from "@/entities/wallet/api/wallet-client";
import type { Position, Redemption, Wallet, Withdrawal } from "@/shared/contracts";
import { cn } from "@/shared/lib/cn";
import { TipAnchor, type TipKey } from "@/shared/tips";
import { formatMoney, formatPct, formatSignedMoney, num, shortAddress } from "@/views/dashboard/lib/format";

const CARD = "rounded-[14px] border border-border bg-main-card";
// The same card, but only from `lg` — on mobile these surfaces sit flat on the page
// (the hero) or hand their chrome to their children (the stat grid).
const CARD_LG = "lg:rounded-[14px] lg:border lg:border-border lg:bg-main-card";
const PALETTE = ["#2a9d8f", "#e58aae", "#2e9e5b", "#f2c94c", "#6ea8fe", "#c084fc"];
const RANGES = ["1M", "6M", "1Y", "All"] as const;

// The portfolio dashboard (Figma `cabinet/home`). Bound to live wallet + fund-position
// data; figures with no backing series yet (the performance chart) are honest, styled
// placeholders rather than fabricated numbers.
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

  const allocations = pos.map((p, i) => ({ name: p.service ?? "Fund", value: num(p.value), color: PALETTE[i % PALETTE.length]! }));
  const allocTotal = allocations.reduce((s, a) => s + a.value, 0) || 1;

  const ops = buildOps(redemptions, withdrawals);

  return (
    // One DOM order, two layouts. Mobile stacks in reading order (hero → figures →
    // what I own → move money → activity); from `xl` the same children are placed
    // explicitly on a two-column grid so the desktop composition is unchanged.
    <div className="grid grid-cols-1 gap-4 px-4 pb-6 pt-5 lg:gap-6 lg:px-8 lg:pb-7 lg:pt-[26px] xl:grid-cols-[minmax(0,1fr)_360px] xl:items-start">
      {/* topbar — desktop only; on mobile the shell app bar plus the hero label carry the page */}
      <div className="hidden items-center justify-between gap-4 lg:flex xl:col-span-2 xl:row-start-1">
        <div className="flex min-w-0 flex-col gap-1">
          <h1 className="font-sans text-2xl font-semibold leading-tight text-foreground">Portfolio</h1>
          <p className="text-[13px] text-main-mist/55">All-time performance and your participation</p>
        </div>
        <div className="flex shrink-0 gap-[10px]">
          <Link href="/wallet/withdraw" className="rounded-lg border border-border px-4 py-[9px] text-[13px] font-semibold text-main-mist/90 transition-colors hover:bg-foreground/[0.04]">
            Withdraw
          </Link>
          <Link href="/wallet/deposit" className="rounded-lg bg-main-accent-t1 px-4 py-[9px] text-[13px] font-semibold text-main-black transition-opacity hover:opacity-90">
            Deposit
          </Link>
        </div>
      </div>

      <PerfCard value={balance?.total} loading={walletLoading} allTimePct={allTimePct} className="lg:order-1 xl:col-start-1 xl:row-span-2 xl:row-start-2" />

      {/* stat strip — a 2×2 card grid on mobile, one divided strip from `lg` */}
      <div className={cn("grid grid-cols-2 gap-3 lg:order-4 lg:flex lg:flex-wrap lg:items-stretch lg:gap-x-7 lg:gap-y-4 lg:px-[26px] lg:py-5", CARD_LG, "xl:col-span-2 xl:col-start-1 xl:row-start-4")}>
        <Stat label="Unrealized P&L" value={walletLoading || posLoading ? null : formatSignedMoney(pnlSum)} tone={pnlSum < 0 ? "loss" : "gain"} hint="across all positions" tip="dashboard.stats.unrealized-pnl" />
        <Divider />
        <Stat label="Available" value={walletLoading ? null : formatMoney(balance?.available)} hint="auto-deploys at EOD" tip="dashboard.stats.available" />
        <Divider />
        <Stat label="Active strategies" value={posLoading ? null : String(pos.length)} hint="fund positions" />
        <Divider />
        <Stat label="Net contributed" value={posLoading ? null : formatMoney(netContributed)} hint="at cost basis" tip="dashboard.stats.net-invested" />
      </div>

      {/* Below `xl` the DOM order is the mobile order; `lg:order-*` restores the desktop
          sequence for the single-column band between `lg` and `xl`. */}
      <WhatIOwn allocations={allocations} total={allocTotal} loading={posLoading} className="lg:order-3 xl:col-start-2 xl:row-start-3" />
      <MoveMoney className="lg:order-2 xl:col-start-2 xl:row-start-2" />

      {/* operations */}
      <div className={cn(CARD, "px-4 pb-2 pt-[18px] lg:order-5 lg:px-[22px] lg:pt-5", "xl:col-span-2 xl:col-start-1 xl:row-start-5")}>
        <div className="flex items-center justify-between pb-1.5">
          <p className="text-[15px] font-semibold text-foreground">Recent operations</p>
          <Link href="/operations" className="text-[13px] text-main-accent-t1 hover:underline">
            View all
          </Link>
        </div>
        {ops.length === 0 ? (
          <p className="py-8 text-center text-[13px] text-main-mist/45 lg:py-10 lg:text-sm">No operations yet — your subscriptions, redemptions and withdrawals will show up here.</p>
        ) : (
          ops.map((op, i) => (
            <div key={op.id} className={cn("flex items-center gap-3 py-3 lg:gap-4 lg:py-[15px]", i > 0 && "border-t border-border")}>
              <span className={cn("shrink-0 rounded-md px-[9px] py-1 text-[11px] font-semibold", op.tagClass)}>{op.tag}</span>
              <div className="flex min-w-0 flex-1 flex-col gap-[3px]">
                <p className="truncate text-[13px] font-semibold text-main-mist lg:text-sm">{op.title}</p>
                <p className="truncate text-[11px] text-main-mist/45 lg:text-xs">{op.sub}</p>
              </div>
              <p className={cn("shrink-0 text-sm font-semibold tabular-nums lg:text-[15px]", op.amountClass)}>{op.amount}</p>
            </div>
          ))
        )}
      </div>
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
    <div className={cn("flex flex-1 flex-col gap-4 lg:gap-[18px] lg:px-[22px] lg:py-5", CARD_LG, className)}>
      <div className="flex flex-col gap-3.5 lg:flex-row lg:items-start lg:justify-between lg:gap-4">
        <div className="flex min-w-0 flex-col gap-2">
          <p className="flex items-center gap-1.5 text-[11px] font-medium tracking-[0.5px] text-main-accent-t1/85">
            PORTFOLIO VALUE
            <TipAnchor anchor="dashboard.performance.portfolio-value" />
          </p>
          <div className="flex flex-col items-start gap-2.5 lg:flex-row lg:items-center lg:gap-[14px]">
            {loading ? <Skeleton className="h-10 w-40 lg:h-12 lg:w-48" /> : <p className="text-[38px] font-semibold leading-none text-white tabular-nums lg:text-[46px]">{formatMoney(value)}</p>}
            {allTimePct !== null && (
              <span className={cn("flex items-center gap-1 rounded-full py-[5px] pl-[9px] pr-[11px] text-xs font-semibold", down ? "bg-destructive/15 text-destructive" : "bg-main-accent-t3/15 text-main-accent-t3")}>
                {down ? <TrendingDown className="size-[13px]" /> : <TrendingUp className="size-[13px]" />}
                {formatPct(allTimePct)} all-time
                <TipAnchor anchor="dashboard.performance.all-time-return" />
              </span>
            )}
          </div>
        </div>
        <div className="grid shrink-0 grid-cols-4 gap-0.5 rounded-[9px] border border-border bg-main-surface p-[3px] lg:flex">
          {RANGES.map((r) => (
            <button
              key={r}
              type="button"
              aria-pressed={r === range}
              onClick={() => setRange(r)}
              className={cn("rounded-[7px] py-2 text-[13px] transition-colors lg:px-[13px] lg:py-1.5 lg:text-xs", r === range ? "bg-main-accent-t1/18 font-semibold text-main-accent-t1" : "font-medium text-main-mist/60 hover:text-foreground")}
            >
              {r}
            </button>
          ))}
        </div>
      </div>
      <div className={cn("flex flex-col gap-3 p-3 lg:gap-[18px] lg:rounded-none lg:border-0 lg:bg-transparent lg:p-0", CARD)}>
        <div className="flex flex-wrap gap-x-[18px] gap-y-1.5 lg:order-2">
          <Legend color="#f2c94c" label="Fund performance" />
          <Legend color="#2e9e5b" label="Your participation" />
        </div>
        {/* Performance series is not yet wired — a styled empty plot rather than a fake line. */}
        <div className="relative h-[164px] w-full overflow-hidden rounded-lg lg:order-1 lg:h-[232px]">
          <div className="absolute inset-x-0 top-1/4 h-px bg-border/60" />
          <div className="absolute inset-x-0 top-1/2 h-px bg-border/60" />
          <div className="absolute inset-x-0 top-3/4 h-px bg-border/60" />
          <div className="absolute inset-0 bg-gradient-to-b from-main-accent-t1/[0.06] to-transparent" />
          <div className="absolute inset-0 flex items-center justify-center">
            <p className="px-4 text-center text-xs text-main-mist/45">Performance history will appear here</p>
          </div>
        </div>
      </div>
    </div>
  );
}

function Legend({ color, label }: { color: string; label: string }) {
  return (
    <span className="flex items-center gap-[7px] text-xs font-medium text-main-mist/60">
      <span className="size-2 rounded-full" style={{ backgroundColor: color }} />
      {label}
    </span>
  );
}

function MoveMoney({ className }: { className?: string }) {
  const [auto, setAuto] = useState(true);
  return (
    <div className={cn(CARD, "flex flex-col gap-3.5 px-4 py-4 lg:gap-4 lg:px-[22px] lg:py-5", className)}>
      <p className="text-[15px] font-semibold text-white">Move money</p>
      <div className="flex gap-[10px]">
        <Link href="/wallet/deposit" className="flex flex-1 items-center justify-center rounded-lg bg-main-accent-t1 px-4 py-2.5 text-[13px] font-semibold text-main-black transition-opacity hover:opacity-90">
          Deposit
        </Link>
        <Link href="/wallet/withdraw" className="flex flex-1 items-center justify-center rounded-lg border border-border px-4 py-2.5 text-[13px] font-semibold text-main-mist/90 transition-colors hover:bg-foreground/[0.04]">
          Withdraw
        </Link>
      </div>
      <div className="flex items-center gap-3 rounded-[10px] border border-border bg-main-surface px-[14px] py-3">
        <div className="flex min-w-0 flex-1 flex-col gap-[3px]">
          <p id="auto-deploy-label" className="flex items-center gap-1.5 text-[13px] font-semibold text-main-mist">
            Auto-deploy idle cash
            <TipAnchor anchor="dashboard.move-money.auto-deploy" />
          </p>
          <p className="text-[11px] text-main-mist/50">Available balance commits at end of day</p>
        </div>
        <button
          type="button"
          role="switch"
          onClick={() => setAuto((v) => !v)}
          aria-checked={auto}
          aria-labelledby="auto-deploy-label"
          className={cn("relative h-5 w-8 shrink-0 rounded-full transition-colors", auto ? "bg-primary" : "bg-input")}
        >
          <span className={cn("absolute top-0.5 size-4 rounded-full bg-primary-foreground transition-all", auto ? "left-[14px]" : "left-0.5")} />
        </button>
      </div>
    </div>
  );
}

function WhatIOwn({ allocations, total, loading, className }: { allocations: { name: string; value: number; color: string }[]; total: number; loading: boolean; className?: string }) {
  return (
    <div className={cn(CARD, "flex flex-col gap-3.5 px-4 py-4 lg:gap-4 lg:px-[22px] lg:py-5", className)}>
      <div className="flex items-center justify-between">
        <p className="flex items-center gap-1.5 text-[15px] font-semibold text-white">
          Invested · what I own
          <TipAnchor anchor="dashboard.invested.allocation" />
        </p>
        <p className="text-xs font-medium text-main-mist/50">
          {allocations.length} {allocations.length === 1 ? "strategy" : "strategies"}
        </p>
      </div>
      {loading ? (
        <Skeleton className="h-24 w-full" />
      ) : allocations.length === 0 ? (
        <p className="py-4 text-sm text-main-mist/45">No fund positions yet.</p>
      ) : (
        <div className="flex flex-col gap-4">
          {allocations.map((a, i) => {
            const pct = Math.round((a.value / total) * 100);
            return (
              <div key={`${a.name}-${i}`} className="flex flex-col gap-[9px]">
                <div className="flex items-center">
                  <span className="flex flex-1 items-center gap-2">
                    <span className="size-[9px] rounded-full" style={{ backgroundColor: a.color }} />
                    <span className="truncate text-[13px] font-medium text-main-mist/90">{a.name}</span>
                  </span>
                  <span className="text-[13px] font-semibold text-white tabular-nums">{pct}%</span>
                </div>
                <div className="h-1.5 w-full overflow-hidden rounded-full bg-border">
                  <div className="h-full rounded-full" style={{ width: `${pct}%`, backgroundColor: a.color }} />
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

function Stat({ label, value, tone, hint, tip }: { label: string; value: string | null; tone?: "gain" | "loss"; hint: string; tip?: TipKey }) {
  const valueClass = tone === "gain" ? "text-main-accent-t2" : tone === "loss" ? "text-destructive" : "text-white";
  const hintClass = tone === "gain" ? "text-main-accent-t2/80" : tone === "loss" ? "text-destructive/80" : "text-main-mist/50";
  return (
    // Its own tile on mobile, a cell of the shared strip from `lg`.
    <div className={cn(CARD, "flex min-w-0 flex-1 flex-col gap-1 px-3.5 py-3 lg:min-w-[120px] lg:gap-1.5 lg:rounded-none lg:border-0 lg:bg-transparent lg:p-0")}>
      <div className="flex items-center gap-1.5">
        <p className="truncate text-[11px] font-medium text-main-mist/55 lg:text-xs">{label}</p>
        {tip && <TipAnchor anchor={tip} />}
      </div>
      {value === null ? <Skeleton className="h-6 w-20" /> : <p className={cn("truncate text-[20px] font-semibold tabular-nums lg:text-[23px]", valueClass)}>{value}</p>}
      <p className={cn("truncate text-[11px] font-medium", hintClass)}>{hint}</p>
    </div>
  );
}

function Divider() {
  return <div className="hidden w-px self-stretch bg-border lg:block" />;
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
