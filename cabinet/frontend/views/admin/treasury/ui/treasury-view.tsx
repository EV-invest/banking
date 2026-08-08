"use client";

import { Check, Copy, RefreshCw, TriangleAlert } from "lucide-react";
import { type ReactNode, useCallback, useEffect, useState } from "react";

import { Button, Card, CardContent, Skeleton } from "@evinvest/uikit";

import { fetchTreasury } from "@/entities/admin/api/admin-client";
import type { RailLiquidity, Treasury } from "@/shared/contracts/admin";
import { displayAddress } from "@/shared/lib/ton-address";
import { TipAnchor, type TipKey } from "@/shared/tips";
import { formatUsd } from "@/views/admin/lib/format";
import { AdminHeader } from "@/views/admin/ui/shell";

const RAIL_LABELS: Record<string, string> = {
  bep20: "BEP20 · BNB Chain",
  trc20: "TRC20 · TRON",
  ton: "TON · Open Network",
  polygon: "Polygon · PoS",
};

const GAS_SYMBOLS: Record<string, string> = {
  bep20: "BNB",
  trc20: "TRX",
  ton: "TON",
  polygon: "POL",
};

export function TreasuryView() {
  // One record so a resolution lands in a single update: a failed read must STOP the
  // skeletons — pulsing placeholders beside an error read as "still coming", so the retry
  // never gets clicked — and it must not leave a stale figure on screen either.
  const [{ treasury, error, loading }, setState] = useState<{ treasury: Treasury | null; error: string | null; loading: boolean }>({
    treasury: null,
    error: null,
    loading: true,
  });
  const [attempt, setAttempt] = useState(0);

  // Retry (event handler — the spinner may start synchronously here; the effect below
  // only ever sets state from its async callbacks).
  const retry = useCallback(() => {
    setState((s) => ({ ...s, loading: true }));
    setAttempt((n) => n + 1);
  }, []);

  useEffect(() => {
    let active = true;
    fetchTreasury()
      .then((t) => active && setState({ treasury: t, error: null, loading: false }))
      .catch((e: Error) => active && setState((s) => ({ ...s, error: e.message, loading: false })));
    return () => {
      active = false;
    };
  }, [attempt]);

  return (
    <div className="space-y-8 px-8 pb-10 pt-6">
      <AdminHeader
        eyebrow="Administer"
        title="Treasury"
        subtitle="Two layers — user claims (USDT) vs on-chain liquidity by rail"
        action={
          <Button type="button" variant="outline" size="sm" disabled={loading} onClick={retry}>
            <RefreshCw className={loading ? "size-4 animate-spin" : "size-4"} /> Refresh
          </Button>
        }
      />

      {error && (
        <div className="flex flex-wrap items-center gap-3 rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2.5">
          <p className="flex items-center gap-2 text-sm text-destructive">
            <TriangleAlert className="size-4 shrink-0" /> {error}
          </p>
          <Button type="button" variant="outline" size="sm" disabled={loading} onClick={retry}>
            <RefreshCw className={loading ? "size-4 animate-spin" : "size-4"} /> Try again
          </Button>
        </div>
      )}

      <section className="space-y-3">
        <p className="text-xs font-semibold uppercase tracking-widest text-muted-foreground">Layer 1 · Ledger — user claims (USDT)</p>
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
          <MoneyCard label="Claims · total (USDT)" value={treasury?.total_custody} hint="= on-chain custody · backed" loading={loading && !treasury} unavailable={!loading && !treasury} tip="admin.treasury.layer1.claims-total" />
          <MoneyCard label="Held for clients" value={treasury?.held_for_clients} hint="user + service balances" loading={loading && !treasury} unavailable={!loading && !treasury} tip="admin.treasury.layer1.held-for-clients" />
          <MoneyCard label="Fund capital" value={treasury?.fund_capital} hint="company's own" loading={loading && !treasury} unavailable={!loading && !treasury} tip="admin.treasury.layer1.fund-capital" />
          <MoneyCard label="Reserved · withdrawals" value={treasury?.reserved_for_withdrawals} hint="queued + in-flight (clearing)" loading={loading && !treasury} unavailable={!loading && !treasury} tip="admin.treasury.layer1.reserved-withdrawals" />
        </div>
      </section>

      <section className="space-y-3">
        <p className="text-xs font-semibold uppercase tracking-widest text-muted-foreground">Layer 2 · Treasury — liquidity by rail</p>
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
          {treasury ? (
            <>
              {treasury.rails.map((rail) => (
                <MoneyCard key={rail.network} label={RAIL_LABELS[rail.network] ?? rail.network} value={rail.custody} loading={false} footer={<RailFunding rail={rail} />} />
              ))}
              <MoneyCard label="Bank · USD" value={treasury.bank} hint="off-ramp · FX" loading={false} tip="admin.treasury.bank" />
            </>
          ) : (
            Array.from({ length: 4 }).map((_, i) => <MoneyCard key={i} label="" value={undefined} loading={loading} unavailable={!loading} />)
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

/** `unavailable` is the read-failed state: a muted dash, never a formatted `$0.00` —
 *  a zero the treasury never reported would be read as a real balance. */
function MoneyCard({ label, value, hint, loading, unavailable, footer, tip }: { label: string; value: string | undefined; hint?: string; loading: boolean; unavailable?: boolean; footer?: ReactNode; tip?: TipKey }) {
  return (
    <Card>
      <CardContent className="space-y-1 py-5">
        <div className="flex items-center gap-1.5">
          <p className="text-xs text-muted-foreground">{label || "…"}</p>
          {tip && <TipAnchor anchor={tip} />}
        </div>
        {loading ? (
          <Skeleton className="mt-1 h-8 w-28" />
        ) : unavailable ? (
          <p className="text-3xl font-semibold tabular-nums text-muted-foreground">—</p>
        ) : (
          <p className="text-3xl font-semibold tabular-nums">{formatUsd(value)}</p>
        )}
        {hint && !loading && !unavailable && <p className="text-xs text-main-accent-t2">{hint}</p>}
        {footer && !loading && footer}
      </CardContent>
    </Card>
  );
}

/** The rail's hot-wallet funding picture — address + on-chain USDT/gas, "—" when the
 * treasury read was unavailable (the hub degrades to empty, never fails). */
function RailFunding({ rail }: { rail: RailLiquidity }) {
  const gasSymbol = GAS_SYMBOLS[rail.network] ?? "";
  // The hub stores TON addresses raw (`workchain:hex`) — an operator can't recognise or
  // paste that into a wallet, so render the same friendly form the deposit screen shows.
  const show = (address: string) => displayAddress(rail.network, address, { testnet: rail.is_testnet });

  return (
    <div className="space-y-2 border-t border-border pt-2.5">
      {rail.treasury_address ? (
        <div className="space-y-1">
          <div className="flex items-center gap-1.5">
            <p className="text-xs text-muted-foreground">Treasury</p>
            <TipAnchor anchor="admin.treasury.rail.address" />
          </div>
          <CopyableAddress address={show(rail.treasury_address)} />
        </div>
      ) : (
        <p className="text-xs text-muted-foreground">— · custody unconfigured</p>
      )}
      <FundingRow label="On-chain USDT" value={rail.onchain_usdt ? qty(rail.onchain_usdt) : undefined} />
      <FundingRow label="Gas" value={rail.onchain_gas ? `${qty(rail.onchain_gas)} ${gasSymbol}`.trimEnd() : undefined} />
      {rail.gas_station_address && (
        <div className="space-y-1.5">
          <div className="flex items-center gap-1.5">
            <p className="text-xs text-muted-foreground">
              Gas station <span className="text-main-accent-t2">(fund {gasSymbol || "gas"} here — pays sweep gas drops)</span>
            </p>
            <TipAnchor anchor="admin.treasury.rail.gas-station" />
          </div>
          <CopyableAddress address={show(rail.gas_station_address)} />
          <FundingRow
            label="Gas station balance"
            value={rail.gas_station_gas ? `${qty(rail.gas_station_gas)} ${gasSymbol}`.trimEnd() : undefined}
          />
        </div>
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

/** Address row with full address in a code block + copy button.
 *  Follows the same pattern as deposit-view's deposit address. */
function CopyableAddress({ address, label }: { address: string; label?: string }) {
  const [copied, setCopied] = useState(false);

  const copy = useCallback(() => {
    void navigator.clipboard.writeText(address);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  }, [address]);

  return (
    <div className="space-y-1">
      {label && <p className="text-xs text-muted-foreground">{label}</p>}
      <div className="flex items-center gap-1.5">
        <code className="flex-1 min-w-0 truncate rounded border border-border bg-main-surface px-2 py-1 font-mono-tech text-xs text-muted-foreground" title={address}>
          {address}
        </code>
        <Button type="button" variant="outline" size="icon" onClick={copy} aria-label={`Copy ${label ?? "address"}`}>
          {copied ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
        </Button>
      </div>
    </div>
  );
}

/** A native-unit decimal string → grouped display; 6 dp so a thin gas balance
 * (e.g. 0.005 BNB) doesn't round to nothing. */
function qty(value: string): string {
  const n = Number(value);
  if (!Number.isFinite(n)) return value;
  return n.toLocaleString("en-US", { maximumFractionDigits: 6 });
}
