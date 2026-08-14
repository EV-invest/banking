"use client";

import { Check, Copy, Loader2, RefreshCw, TriangleAlert } from "lucide-react";
import { type ReactNode, useCallback, useState } from "react";

import { Button, Card, CardContent, Input, Select, SelectContent, SelectItem, SelectTrigger, Skeleton } from "@evinvest/uikit";

import { recordTreasuryDeposit, type RecordedArrival } from "@/entities/admin/api/admin-client";
import { treasuryResource } from "@/entities/admin/model/admin-resource";
import type { RailLiquidity } from "@/shared/contracts/admin";
import { cn } from "@/shared/lib/cn";
import { useResource } from "@/shared/lib/resource";
import { displayAddress } from "@/shared/lib/ton-address";
import { TipAnchor, type TipKey } from "@/shared/tips";
import { formatUsd } from "@/views/admin/lib/format";
import { AdminHeader } from "@/views/admin/ui/shell";

const TEAL_CTA = "bg-main-accent-t1 text-main-black hover:bg-main-accent-t1/90";

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
  // Cached, so returning from another console screen paints the figures immediately and
  // refreshes them behind. A failed read must STOP the skeletons — pulsing placeholders
  // beside an error read as "still coming", so the retry never gets clicked — which is
  // exactly what `isLoading` reports: false once an attempt has settled either way.
  const read = useResource(treasuryResource);
  const treasury = read.data ?? null;
  const error = read.error?.message ?? null;
  const loading = read.isLoading || read.isValidating;
  const retry = () => void read.refresh();

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

      <RecordArrival rails={treasury?.rails} onRecorded={retry} />

      <p className="max-w-3xl text-xs text-muted-foreground">
        Per-rail backing is the treasury&apos;s job, not the ledger&apos;s: a shortfall on one rail is accept-and-queue, then rebalanced via CEX / alt-rail / top-up. The global invariant is{" "}
        <span className="font-mono-tech">sum(custody) == sum(claims)</span>.
      </p>
    </div>
  );
}

/** Funding a treasury hot wallet directly moves real USDT while writing nothing to the
 *  ledger: the rail's custody figure doesn't move, fund capital understates what went in,
 *  and the dispatch gate (`min(TB rail, on-chain treasury)`) keeps reading the old number,
 *  so that liquidity can't be withdrawn. This is where that arrival gets recorded.
 *
 *  Idempotent by `tx_ref`, so the honest outcome is three-way: credited, already credited,
 *  or failed — collapsing "already credited" into a generic success would invite the
 *  operator to re-submit under a second reference and double-count the same dollar. */
function RecordArrival({ rails, onRecorded }: { rails: RailLiquidity[] | undefined; onRecorded: () => void }) {
  const [network, setNetwork] = useState("");
  const [txRef, setTxRef] = useState("");
  const [amount, setAmount] = useState("");
  const [state, setState] = useState<{ busy: boolean; error: string | null; result: RecordedArrival | null }>({ busy: false, error: null, result: null });

  // Only rails the hub actually reported: an address minted for a rail nothing watches is
  // exactly the mistake this screen exists to prevent.
  const options = rails?.filter((r) => r.treasury_address) ?? [];

  const submit = useCallback(() => {
    setState({ busy: true, error: null, result: null });
    // The amount goes as an ASSERTION, and only when the operator typed one — the hub reads
    // the real figure off the chain. Sending it as a value is what would let this mint money.
    recordTreasuryDeposit({ tx_ref: txRef.trim(), network, expected_amount: amount.trim() || undefined })
      .then((res) => {
        setState({ busy: false, error: null, result: res });
        if (res.recorded) {
          setTxRef("");
          setAmount("");
          onRecorded();
        }
      })
      .catch((e: Error) => setState({ busy: false, error: e.message, result: null }));
  }, [txRef, network, amount, onRecorded]);

  return (
    <section className="space-y-3">
      <p className="text-xs font-semibold uppercase tracking-widest text-muted-foreground">Record an out-of-band arrival</p>
      <Card>
        <CardContent className="space-y-5 py-6">
          <p className="max-w-3xl text-sm text-muted-foreground">
            USDT sent straight to a treasury hot wallet is real money the ledger never saw. Give its on-chain reference and the hub reads the transfer back off the chain — the
            amount and the party credited come from there, so this records an arrival rather than asserting one.
          </p>
          <div className="grid gap-4 md:grid-cols-3">
            <div className="flex flex-col gap-1.5">
              <span className="text-sm text-muted-foreground">Rail</span>
              <Select value={network} onValueChange={setNetwork}>
                <SelectTrigger className="w-full border-border bg-main-surface" disabled={options.length === 0}>
                  {/* The placeholder is trigger text, not a selectable item — "Select a
                      rail…" is not a rail. */}
                  <span className={cn("truncate", !network && "text-muted-foreground")}>
                    {network ? (RAIL_LABELS[network] ?? network) : options.length === 0 ? "No rail with a treasury" : "Select a rail…"}
                  </span>
                </SelectTrigger>
                <SelectContent>
                  {options.map((r) => (
                    <SelectItem key={r.network} value={r.network}>
                      {RAIL_LABELS[r.network] ?? r.network}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <label className="flex flex-col gap-1.5">
              <span className="text-sm text-muted-foreground">Expected amount (USDT) · optional</span>
              <Input value={amount} onChange={(e) => setAmount(e.target.value)} inputMode="decimal" placeholder="any" className="w-full" />
            </label>
            <label className="flex flex-col gap-1.5">
              <span className="text-sm text-muted-foreground">On-chain reference</span>
              <Input value={txRef} onChange={(e) => setTxRef(e.target.value)} placeholder="0xhash:logIndex" className="w-full" />
            </label>
          </div>

          <p className="text-xs text-muted-foreground">
            The reference is both what gets verified and the idempotency key — <span className="font-mono-tech">txhash:logIndex</span> on an EVM rail,{" "}
            <span className="font-mono-tech">txhash:piggybank</span> on TON. A reference that names no confirmed transfer to one of our addresses is refused, and a
            re-submission of one already recorded is a no-op. Fill the amount only to assert what you expect; a mismatch is then an error instead of a surprise.
          </p>

          {state.error && (
            <p className="flex items-center gap-2 text-sm text-destructive">
              <TriangleAlert className="size-4 shrink-0" /> {state.error}
            </p>
          )}
          {state.result?.recorded && (
            <p className="text-sm text-main-accent-t1">
              Recorded — the chain reported <span className="font-semibold tabular-nums">{formatUsd(state.result.amount)}</span>, credited to {partyLabel(state.result)}.
            </p>
          )}
          {state.result && !state.result.recorded && (
            <p className="text-sm text-main-accent-t3">Already recorded — that reference was credited before, nothing changed.</p>
          )}

          <Button type="button" className={cn("ml-auto flex", TEAL_CTA)} disabled={state.busy || !network || !txRef.trim()} onClick={submit}>
            {state.busy ? <Loader2 className="size-4 animate-spin" /> : null}
            Record arrival
          </Button>
        </CardContent>
      </Card>
    </section>
  );
}

/** Who the chain said the money belongs to. Worth showing rather than assuming the fund:
 *  a reference that turns out to be a user's deposit credits that user, and the operator
 *  should see that happened instead of reading it as company capital. */
function partyLabel({ party_kind, party_id }: RecordedArrival): string {
  if (party_kind === "piggybank") return "fund capital";
  return party_id ? `${party_kind} ${party_id}` : party_kind;
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
