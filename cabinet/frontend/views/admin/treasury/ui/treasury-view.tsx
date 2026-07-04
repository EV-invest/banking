"use client";

import { TriangleAlert } from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";

import { Card, CardContent, Skeleton } from "@evinvest/uikit";

import { fetchTreasury } from "@/entities/admin/api/admin-client";
import type { RailLiquidity, Treasury } from "@/shared/contracts/admin";
import { usd } from "@/views/admin/lib/format";
import { AdminHeader } from "@/views/admin/ui/shell";

const RAIL_LABELS: Record<string, string> = {
  bep20: "BEP20 · BNB Chain",
  trc20: "TRC20 · TRON",
  ton: "TON · Open Network",
};

const GAS_SYMBOLS: Record<string, string> = {
  bep20: "BNB",
  trc20: "TRX",
  ton: "TON",
};

export function TreasuryView() {
  const [treasury, setTreasury] = useState<Treasury | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    fetchTreasury()
      .then((t) => active && setTreasury(t))
      .catch((e: Error) => active && setError(e.message));
    return () => {
      active = false;
    };
  }, []);

  return (
    <div className="space-y-8 px-8 pb-10 pt-6">
      <AdminHeader eyebrow="Administer" title="Treasury" subtitle="Two layers — user claims (USDT) vs on-chain liquidity by rail" />

      {error && (
        <p className="flex items-center gap-2 text-sm text-destructive">
          <TriangleAlert className="size-4" /> {error}
        </p>
      )}

      <section className="space-y-3">
        <p className="text-[11px] font-semibold uppercase tracking-widest text-muted-foreground">Layer 1 · Ledger — user claims (USDT)</p>
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
          <MoneyCard label="Claims · total (USDT)" value={treasury?.total_custody} hint="= on-chain custody · backed" loading={!treasury} />
          <MoneyCard label="Held for clients" value={treasury?.held_for_clients} hint="user + service balances" loading={!treasury} />
          <MoneyCard label="Fund capital" value={treasury?.fund_capital} hint="company's own" loading={!treasury} />
          <MoneyCard label="Reserved · withdrawals" value={treasury?.reserved_for_withdrawals} hint="queued + in-flight (clearing)" loading={!treasury} />
        </div>
      </section>

      <section className="space-y-3">
        <p className="text-[11px] font-semibold uppercase tracking-widest text-muted-foreground">Layer 2 · Treasury — liquidity by rail</p>
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
          {treasury ? (
            <>
              {treasury.rails.map((rail) => (
                <MoneyCard key={rail.network} label={RAIL_LABELS[rail.network] ?? rail.network} value={rail.custody} loading={false} footer={<RailFunding rail={rail} />} />
              ))}
              <MoneyCard label="Bank · USD" value={treasury.bank} hint="off-ramp · FX" loading={false} />
            </>
          ) : (
            Array.from({ length: 4 }).map((_, i) => <MoneyCard key={i} label="" value={undefined} loading />)
          )}
        </div>
      </section>

      <p className="max-w-3xl text-xs text-muted-foreground">
        Per-rail backing is the treasury&apos;s job, not the ledger&apos;s: a shortfall on one rail is accept-and-queue, then rebalanced via CEX / alt-rail / top-up. The global invariant is{" "}
        <span className="font-mono-tech">sum(custody) == sum(claims)</span>.
      </p>
    </div>
  );
}

function MoneyCard({ label, value, hint, loading, footer }: { label: string; value: string | undefined; hint?: string; loading: boolean; footer?: ReactNode }) {
  return (
    <Card>
      <CardContent className="space-y-1 py-5">
        <p className="text-xs text-muted-foreground">{label || "…"}</p>
        {loading ? <Skeleton className="mt-1 h-8 w-28" /> : <p className="text-3xl font-semibold tabular-nums">{usd(value)}</p>}
        {hint && !loading && <p className="text-xs text-main-accent-t2">{hint}</p>}
        {footer && !loading && footer}
      </CardContent>
    </Card>
  );
}

/** The rail's hot-wallet funding picture — address + on-chain USDT/gas, "—" when the
 * treasury read was unavailable (the hub degrades to empty, never fails). */
function RailFunding({ rail }: { rail: RailLiquidity }) {
  return (
    <div className="space-y-1 border-t border-border pt-2">
      {rail.treasury_address ? (
        <p className="font-mono-tech text-xs text-muted-foreground" title={rail.treasury_address}>
          {shortAddr(rail.treasury_address)}
        </p>
      ) : (
        <p className="text-xs text-muted-foreground">— · custody unconfigured</p>
      )}
      <FundingRow label="On-chain USDT" value={rail.onchain_usdt ? qty(rail.onchain_usdt) : undefined} />
      <FundingRow label="Gas" value={rail.onchain_gas ? `${qty(rail.onchain_gas)} ${GAS_SYMBOLS[rail.network] ?? ""}`.trimEnd() : undefined} />
      {rail.gas_station_address && (
        <>
          <p className="pt-1 text-xs text-muted-foreground">
            Gas station <span className="text-main-accent-t2">(fund {GAS_SYMBOLS[rail.network] ?? "gas"} here — pays sweep gas drops)</span>
          </p>
          <p className="font-mono-tech text-xs text-muted-foreground" title={rail.gas_station_address}>
            {shortAddr(rail.gas_station_address)}
          </p>
          <FundingRow
            label="Gas station balance"
            value={rail.gas_station_gas ? `${qty(rail.gas_station_gas)} ${GAS_SYMBOLS[rail.network] ?? ""}`.trimEnd() : undefined}
          />
        </>
      )}
    </div>
  );
}

function FundingRow({ label, value }: { label: string; value: string | undefined }) {
  return (
    <div className="flex items-center justify-between text-xs">
      <span className="text-muted-foreground">{label}</span>
      <span className="tabular-nums">{value ?? "—"}</span>
    </div>
  );
}

function shortAddr(address: string): string {
  return address.length > 18 ? `${address.slice(0, 8)}…${address.slice(-6)}` : address;
}

/** A native-unit decimal string → grouped display; 6 dp so a thin gas balance
 * (e.g. 0.005 BNB) doesn't round to nothing. */
function qty(value: string): string {
  const n = Number(value);
  if (!Number.isFinite(n)) return value;
  return n.toLocaleString("en-US", { maximumFractionDigits: 6 });
}
