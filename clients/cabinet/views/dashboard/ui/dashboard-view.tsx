"use client";

import { ArrowDownToLine, ArrowUpFromLine, TrendingDown, TrendingUp } from "lucide-react";
import Link from "next/link";
import { useEffect, useState } from "react";

import { Skeleton } from "@evinvest/uikit";

import { fetchPositions, fetchRedemptions } from "@/entities/fund/api/fund-client";
import { fetchWallet, fetchWithdrawals } from "@/entities/wallet/api/wallet-client";
import type { Position, Redemption, Wallet, Withdrawal } from "@/shared/contracts";
import { cn } from "@/shared/lib/cn";
import { formatMoney, formatPct, formatSignedMoney, num, shortAddress } from "@/views/dashboard/lib/format";

const CARD = "rounded-[14px] border border-border bg-main-card";
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
    <div className="flex flex-col gap-6 px-8 pb-7 pt-6">
      {/* topbar */}
      <div className="flex items-center justify-between gap-4">
        <div className="min-w-0">
          <h1 className="font-sans text-2xl font-semibold text-foreground">Portfolio</h1>
          <p className="text-[13px] text-muted-foreground">All-time performance and your participation</p>
        </div>
        <div className="flex shrink-0 gap-[10px]">
          <Link href="/wallet" className="rounded-lg border border-border px-4 py-[9px] text-[13px] font-semibold text-main-mist/90 transition-colors hover:bg-foreground/[0.04]">
            Withdraw
          </Link>
          <Link href="/wallet" className="rounded-lg bg-main-accent-t1 px-4 py-[9px] text-[13px] font-semibold text-main-black transition-opacity hover:opacity-90">
            Deposit
          </Link>
        </div>
      </div>

      {/* heroRow */}
      <div className="flex flex-col gap-6 xl:flex-row">
        <PerfCard value={balance?.total} loading={walletLoading} allTimePct={allTimePct} />
        <div className="flex w-full flex-col gap-6 xl:w-[360px]">
          <MoveMoney />
          <WhatIOwn allocations={allocations} total={allocTotal} loading={posLoading} />
        </div>
      </div>

      {/* stat strip */}
      <div className={cn(CARD, "flex flex-wrap items-stretch gap-x-7 gap-y-4 px-[26px] py-5")}>
        <Stat label="Unrealized P&L" value={walletLoading || posLoading ? null : formatSignedMoney(pnlSum)} tone={pnlSum < 0 ? "loss" : "gain"} hint="across all positions" />
        <Divider />
        <Stat label="Available" value={walletLoading ? null : formatMoney(balance?.available)} hint="free to deploy" />
        <Divider />
        <Stat label="Active strategies" value={posLoading ? null : String(pos.length)} hint="fund positions" />
        <Divider />
        <Stat label="Net invested" value={posLoading ? null : formatMoney(netContributed)} hint="at cost basis" />
      </div>

      {/* operations */}
      <div className={cn(CARD, "px-[22px] pb-2 pt-5")}>
        <div className="flex items-center justify-between pb-1.5">
          <p className="text-[15px] font-semibold text-foreground">Recent operations</p>
          <Link href="/operations" className="text-[13px] text-main-accent-t1 hover:underline">
            View all
          </Link>
        </div>
        {ops.length === 0 ? (
          <p className="py-10 text-center text-sm text-muted-foreground">No operations yet — your subscriptions, redemptions and withdrawals will show up here.</p>
        ) : (
          ops.map((op, i) => (
            <div key={op.id} className={cn("flex items-center gap-4 py-[15px]", i > 0 && "border-t border-border")}>
              <span className={cn("rounded-md px-[9px] py-1 text-[11px] font-semibold", op.tagClass)}>{op.tag}</span>
              <div className="min-w-0 flex-1">
                <p className="truncate text-sm font-semibold text-main-mist">{op.title}</p>
                <p className="truncate text-xs text-muted-foreground">{op.sub}</p>
              </div>
              <p className={cn("shrink-0 text-[15px] font-semibold tabular-nums", op.amountClass)}>{op.amount}</p>
            </div>
          ))
        )}
      </div>
    </div>
  );
}

function PerfCard({ value, loading, allTimePct }: { value: string | undefined; loading: boolean; allTimePct: number | null }) {
  const [range, setRange] = useState<(typeof RANGES)[number]>("All");
  const down = (allTimePct ?? 0) < 0;
  return (
    <div className={cn(CARD, "flex flex-1 flex-col gap-[18px] px-[22px] py-5")}>
      <div className="flex items-start justify-between gap-4">
        <div className="flex flex-col gap-2">
          <p className="text-[11px] font-medium text-main-accent-t1/85">PORTFOLIO VALUE</p>
          <div className="flex items-center gap-[14px]">
            {loading ? <Skeleton className="h-12 w-48" /> : <p className="text-[46px] font-semibold leading-none text-white tabular-nums">{formatMoney(value)}</p>}
            {allTimePct !== null && (
              <span className={cn("flex items-center gap-1 rounded-full px-2.5 py-1 text-xs font-semibold", down ? "bg-main-accent-t4/15 text-main-accent-t4" : "bg-main-accent-t3/15 text-main-accent-t3")}>
                {down ? <TrendingDown className="size-3.5" /> : <TrendingUp className="size-3.5" />}
                {formatPct(allTimePct)} all-time
              </span>
            )}
          </div>
        </div>
        <div className="flex gap-0.5 rounded-[9px] border border-border bg-main-surface p-[3px]">
          {RANGES.map((r) => (
            <button
              key={r}
              type="button"
              aria-pressed={r === range}
              onClick={() => setRange(r)}
              className={cn("rounded-[7px] px-[13px] py-1.5 text-xs font-medium transition-colors", r === range ? "bg-main-accent-t1/18 text-main-accent-t1" : "text-muted-foreground hover:text-foreground")}
            >
              {r}
            </button>
          ))}
        </div>
      </div>
      {/* Performance series is not yet wired — a styled empty plot rather than a fake line. */}
      <div className="relative h-[232px] w-full overflow-hidden rounded-lg">
        <div className="absolute inset-x-0 top-1/4 h-px bg-border/60" />
        <div className="absolute inset-x-0 top-1/2 h-px bg-border/60" />
        <div className="absolute inset-x-0 top-3/4 h-px bg-border/60" />
        <div className="absolute inset-0 bg-gradient-to-b from-main-accent-t1/[0.06] to-transparent" />
        <div className="absolute inset-0 flex items-center justify-center">
          <p className="text-xs text-muted-foreground">Performance history will appear here</p>
        </div>
      </div>
      <div className="flex gap-[18px]">
        <Legend color="#f2c94c" label="Fund performance" />
        <Legend color="#2a9d8f" label="Your participation" />
      </div>
    </div>
  );
}

function Legend({ color, label }: { color: string; label: string }) {
  return (
    <span className="flex items-center gap-[7px] text-xs text-muted-foreground">
      <span className="size-2 rounded-full" style={{ backgroundColor: color }} />
      {label}
    </span>
  );
}

function MoveMoney() {
  const [auto, setAuto] = useState(true);
  return (
    <div className={cn(CARD, "flex flex-col gap-4 px-[22px] py-5")}>
      <p className="text-[15px] font-semibold text-white">Move money</p>
      <div className="flex gap-[10px]">
        <Link href="/wallet" className="flex flex-1 items-center justify-center gap-2 rounded-lg bg-main-accent-t1 px-4 py-2.5 text-[13px] font-semibold text-main-black transition-opacity hover:opacity-90">
          <ArrowDownToLine className="size-4" /> Deposit
        </Link>
        <Link href="/wallet" className="flex flex-1 items-center justify-center gap-2 rounded-lg border border-border px-4 py-2.5 text-[13px] font-semibold text-main-mist/90 transition-colors hover:bg-foreground/[0.04]">
          <ArrowUpFromLine className="size-4" /> Withdraw
        </Link>
      </div>
      <div className="flex items-center gap-3 rounded-[10px] border border-border bg-main-surface px-[14px] py-3">
        <div className="min-w-0 flex-1">
          <p id="auto-deploy-label" className="text-[13px] font-semibold text-main-mist">
            Auto-deploy idle cash
          </p>
          <p className="text-[11px] text-muted-foreground">Available balance commits at end of day</p>
        </div>
        <button
          type="button"
          role="switch"
          onClick={() => setAuto((v) => !v)}
          aria-checked={auto}
          aria-labelledby="auto-deploy-label"
          className={cn("relative h-5 w-8 shrink-0 rounded-full transition-colors", auto ? "bg-main-accent-t1" : "bg-border")}
        >
          <span className={cn("absolute top-0.5 size-4 rounded-full bg-white transition-all", auto ? "left-[14px]" : "left-0.5")} />
        </button>
      </div>
    </div>
  );
}

function WhatIOwn({ allocations, total, loading }: { allocations: { name: string; value: number; color: string }[]; total: number; loading: boolean }) {
  return (
    <div className={cn(CARD, "flex flex-col gap-4 px-[22px] py-5")}>
      <div className="flex items-center justify-between">
        <p className="text-[15px] font-semibold text-white">Invested · what I own</p>
        <p className="text-xs text-muted-foreground">
          {allocations.length} {allocations.length === 1 ? "strategy" : "strategies"}
        </p>
      </div>
      {loading ? (
        <Skeleton className="h-24 w-full" />
      ) : allocations.length === 0 ? (
        <p className="py-4 text-sm text-muted-foreground">No fund positions yet.</p>
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

function Stat({ label, value, tone, hint }: { label: string; value: string | null; tone?: "gain" | "loss"; hint: string }) {
  const valueClass = tone === "gain" ? "text-main-accent-t2" : tone === "loss" ? "text-main-accent-t4" : "text-white";
  return (
    <div className="flex min-w-[120px] flex-1 flex-col gap-1.5">
      <p className="text-xs font-medium text-muted-foreground">{label}</p>
      {value === null ? <Skeleton className="h-6 w-20" /> : <p className={cn("text-[23px] font-semibold tabular-nums", valueClass)}>{value}</p>}
      <p className="text-[11px] text-muted-foreground">{hint}</p>
    </div>
  );
}

function Divider() {
  return <div className="w-px self-stretch bg-border" />;
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
    tagClass: "bg-main-accent-t4/15 text-main-accent-t4",
    title: `Withdrawal · ${(w.network ?? "").toUpperCase()}`,
    sub: `${shortAddress(w.address)} · ${w.state ?? ""}`,
    amount: `−${formatMoney(w.net_amount ?? w.amount)}`,
    amountClass: "text-main-mist",
  }));
  return [...fromRedemptions, ...fromWithdrawals].slice(0, 6);
}
