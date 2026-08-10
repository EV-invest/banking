"use client";

import { ArrowDownToLine, Clock, Loader2, Sparkles, TrendingUp, TriangleAlert, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";

import { Alert, AlertDescription, AlertTitle, Badge, Button, Card, CardContent, Input, Skeleton } from "@evinvest/uikit";

import { cancelRedemption, fetchAllocations, fetchFundNav, fetchPositions, fetchRedemptions, submitRedeem, submitSubscribe } from "@/entities/fund/api/fund-client";
import type { Allocation, FundNav, Position, Redemption } from "@/shared/contracts";
import { cn } from "@/shared/lib/cn";
import { TipAnchor } from "@/shared/tips";
import { formatSignedUsdt, formatUnits, formatUsdt, fromBaseUnits, isNegative, isZero, toBaseUnits } from "@/views/invest/lib/format";

const TEAL_CTA = "bg-main-accent-t1 text-main-black hover:bg-main-accent-t1/90";
const SCALE = 10n ** 18n;

// One row of the screen: a product, plus whatever the caller holds in it. The two used to
// be separate sections, so a holder saw their fund twice — once as a position and once as
// a row in the subscribe form's dropdown.
interface Product {
  service: string;
  title: string;
  summary: string;
  /** Absent when the product is no longer in the open catalog but units are still held. */
  allocation: Allocation | null;
  position: Position | null;
}

/** `floor(cash / nav)` in exact base units — mirrors `Shares::from_cash` on the hub, so
 *  the preview cannot disagree with what the ledger actually mints. */
function unitsForCash(amount: string, nav: string | undefined): bigint | null {
  const cash = toBaseUnits(amount);
  const price = toBaseUnits(nav);
  if (cash <= 0n || price <= 0n) return null;
  return (cash * SCALE) / price;
}

/** `units × nav`, the settle-time estimate shown on redeem. */
function cashForUnits(units: string, nav: string | undefined): bigint | null {
  const held = toBaseUnits(units);
  const price = toBaseUnits(nav);
  if (held <= 0n || price <= 0n) return null;
  return (held * price) / SCALE;
}

export function InvestView() {
  const [positions, setPositions] = useState<Position[] | null>(null);
  const [catalog, setCatalog] = useState<Allocation[] | null>(null);
  const [redemptions, setRedemptions] = useState<Redemption[]>([]);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    fetchPositions()
      .then((list) => {
        setPositions(list.positions ?? []);
        setError(null);
      })
      .catch((e: Error) => setError(e.message));
    fetchRedemptions()
      .then((list) => setRedemptions(list.redemptions ?? []))
      .catch(() => setRedemptions([]));
  }, []);

  useEffect(load, [load]);

  useEffect(() => {
    fetchAllocations()
      .then((list) => setCatalog(list.allocations ?? []))
      .catch(() => setCatalog([]));
  }, []);

  // The open catalog UNION the services already held. A closed product is absent from the
  // catalog but its holders still have units in it — dropping it here would hide real
  // money, and the hub deliberately keeps redeeming it.
  const products = useMemo<Product[] | null>(() => {
    if (!catalog || !positions) return null;
    const byService = new Map<string, Product>();
    for (const a of catalog) {
      byService.set(a.service, { service: a.service, title: a.title, summary: a.summary, allocation: a, position: null });
    }
    for (const p of positions) {
      const service = p.service ?? "";
      if (!service) continue;
      const existing = byService.get(service);
      if (existing) existing.position = p;
      else byService.set(service, { service, title: service, summary: "", allocation: null, position: p });
    }
    return [...byService.values()].sort((a, b) => a.title.localeCompare(b.title));
  }, [catalog, positions]);

  return (
    <div className="container max-w-4xl space-y-8 py-12">
      <header className="space-y-1">
        <p className="font-mono-tech text-xs uppercase tracking-widest text-main-accent-t1">Invest</p>
        <h1 className="flex items-center gap-2 text-3xl font-semibold">
          Your fund shares
          <TipAnchor anchor="invest.overview" />
        </h1>
        {/* The single most misread thing about this product: a holding does not grow in
            unit count. Say it on the screen, not only in a tooltip. */}
        <p className="text-sm text-muted-foreground">Your unit count stays put — it is the NAV per unit that moves, and with it what your units are worth.</p>
      </header>

      {error && (
        <Alert variant="destructive">
          <TriangleAlert className="size-4" />
          <AlertTitle>Couldn&apos;t load your positions</AlertTitle>
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}

      {!products ? (
        <div className="space-y-4">
          <Skeleton className="h-52 w-full" />
          <Skeleton className="h-52 w-full" />
        </div>
      ) : products.length === 0 ? (
        <Card>
          <CardContent className="flex flex-col items-center gap-2 py-16 text-center text-muted-foreground">
            <Sparkles className="size-6" />
            <p className="text-sm">No funds are open for subscription right now.</p>
            <p className="max-w-sm text-xs">A fund appears here once an operator registers and opens it. Nothing you hold is affected.</p>
          </CardContent>
        </Card>
      ) : (
        <div className="space-y-5">
          {products.map((product) => (
            <ProductCard key={product.service} product={product} redemptions={redemptions.filter((r) => r.service === product.service)} onDone={load} />
          ))}
        </div>
      )}
    </div>
  );
}

type Panel = "subscribe" | "redeem" | null;

function ProductCard({ product, redemptions, onDone }: { product: Product; redemptions: Redemption[]; onDone: () => void }) {
  const [nav, setNav] = useState<FundNav | null>(null);
  const [panel, setPanel] = useState<Panel>(null);

  useEffect(() => {
    let active = true;
    fetchFundNav(product.service)
      .then((n) => active && setNav(n))
      .catch(() => active && setNav(null));
    return () => {
      active = false;
    };
  }, [product.service]);

  const held = product.position;
  const closed = product.allocation === null;
  const stale = nav?.stale ?? false;
  // `posted_at` is 0 until an operator marks the fund, which is exactly when the hub is
  // still pricing at the bootstrap NAV of 1.0.
  const unmarked = nav !== null && Number(nav.posted_at ?? 0) === 0;
  const queued = redemptions.filter((r) => r.state === "queued");

  return (
    <Card>
      <CardContent className="space-y-5 py-6">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div className="min-w-0">
            <p className="text-lg font-semibold">{product.title}</p>
            <p className="font-mono-tech text-xs text-muted-foreground">{product.service}</p>
            {product.summary && <p className="mt-1 max-w-md text-sm text-muted-foreground">{product.summary}</p>}
          </div>
          <div className="flex flex-wrap items-center gap-2">
            {closed && (
              <Badge variant="outline" className="gap-1 border-main-accent-t3/40 text-main-accent-t3">
                Redeem only
              </Badge>
            )}
            {stale && (
              <Badge variant="outline" className="gap-1 border-main-accent-t3/40 text-main-accent-t3">
                <Clock className="size-3" /> Stale NAV
                <TipAnchor anchor="invest.position.stale-nav" />
              </Badge>
            )}
          </div>
        </div>

        {held ? <HoldingStats position={held} /> : <PriceOnly nav={nav} unmarked={unmarked} />}

        {/* The gates, stated before the action rather than after a failed submit. */}
        {closed && <Note tone="amber">This fund is closed to new subscriptions. Your units are unaffected — you can still redeem them in full.</Note>}
        {stale && <Note tone="amber">The fund&apos;s valuation is out of date, so dealing is paused until an operator posts a fresh one.</Note>}
        {unmarked && !closed && <Note tone="muted">No valuation posted yet — units price at the bootstrap NAV of 1.0 until the first mark.</Note>}

        <div className="flex flex-wrap gap-2">
          {!closed && (
            <Button type="button" className={cn(TEAL_CTA)} disabled={stale} onClick={() => setPanel((p) => (p === "subscribe" ? null : "subscribe"))}>
              <Sparkles className="size-4" />
              {panel === "subscribe" ? "Close" : "Subscribe"}
            </Button>
          )}
          {held && (
            <Button type="button" variant="outline" disabled={stale} onClick={() => setPanel((p) => (p === "redeem" ? null : "redeem"))}>
              <ArrowDownToLine className="size-4" />
              {panel === "redeem" ? "Close" : "Redeem"}
            </Button>
          )}
        </div>

        {panel === "subscribe" && <SubscribePanel service={product.service} nav={nav} onDone={onDone} />}
        {panel === "redeem" && held && <RedeemPanel service={product.service} position={held} nav={nav} onDone={onDone} />}

        {queued.length > 0 && <QueuedList items={queued} onDone={onDone} />}
      </CardContent>
    </Card>
  );
}

function HoldingStats({ position }: { position: Position }) {
  const loss = isNegative(position.pnl);
  const flat = isZero(position.pnl);
  return (
    <div className="grid gap-3 sm:grid-cols-4">
      <Stat label="Units" value={formatUnits(position.units)} tip="invest.position.units" />
      <Stat label="NAV" value={formatUsdt(position.nav)} tip="invest.position.nav" />
      <Stat label="Value" value={`${formatUsdt(position.value)} USDT`} emphasis tip="invest.position.value" />
      <Stat
        label="P&L"
        value={`${formatSignedUsdt(position.pnl)} USDT`}
        tip="invest.position.pnl"
        tone={loss && !flat ? "text-main-accent-t4" : "text-main-accent-t2"}
        icon={<TrendingUp className={cn("size-3.5", loss && !flat && "rotate-180")} />}
      />
    </div>
  );
}

/** A product the caller holds nothing in: the price is all there is to show. */
function PriceOnly({ nav, unmarked }: { nav: FundNav | null; unmarked: boolean }) {
  return (
    <div className="grid gap-3 sm:grid-cols-4">
      <Stat label="NAV / unit" value={nav ? formatUsdt(nav.nav) : "—"} tip="invest.position.nav" emphasis />
      <div className="sm:col-span-3 flex items-center text-sm text-muted-foreground">{unmarked ? "Not yet valued — the first subscription prices at 1.0." : "You hold no units in this fund yet."}</div>
    </div>
  );
}

function SubscribePanel({ service, nav, onDone }: { service: string; nav: FundNav | null; onDone: () => void }) {
  const [amount, setAmount] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<{ units?: string; nav?: string } | null>(null);

  // Exact preview, floored the same way the hub floors it — so "0 units" is visible here
  // instead of arriving as a rejection.
  const preview = unitsForCash(amount, nav?.nav);
  const dust = toBaseUnits(amount) > 0n && preview === 0n;

  const submit = async () => {
    setSubmitting(true);
    setError(null);
    setDone(null);
    try {
      const receipt = await submitSubscribe({ service, amount });
      setDone({ units: receipt.units, nav: receipt.nav });
      setAmount("");
      onDone();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="space-y-3 rounded-lg border border-border bg-main-surface p-4">
      {done && (
        <Alert>
          <Sparkles className="size-4 text-main-accent-t2" />
          <AlertTitle>Subscription received</AlertTitle>
          <AlertDescription>
            Minted {formatUnits(done.units)} units at {formatUsdt(done.nav)} USDT NAV — your position updates shortly.
          </AlertDescription>
        </Alert>
      )}
      {error && (
        <Alert variant="destructive">
          <TriangleAlert className="size-4" />
          <AlertTitle>Subscription failed</AlertTitle>
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}

      <div className="flex flex-wrap items-end gap-3">
        <label className="flex min-w-48 flex-1 flex-col gap-1.5">
          <span className="flex items-center gap-1.5 text-sm">
            Amount (USDT)
            <TipAnchor anchor="invest.subscribe.amount" />
          </span>
          <Input value={amount} onChange={(e) => setAmount(e.target.value)} inputMode="decimal" placeholder="0.00" className="w-full" />
        </label>
        <Button type="button" className={cn(TEAL_CTA)} disabled={submitting || preview === null || preview === 0n} onClick={submit}>
          {submitting ? <Loader2 className="size-4 animate-spin" /> : <Sparkles className="size-4" />}
          Subscribe
        </Button>
      </div>

      <p className={cn("text-xs", dust ? "text-destructive" : "text-muted-foreground")}>
        {dust
          ? `Too small to buy a whole unit at ${formatUsdt(nav?.nav)} — increase the amount.`
          : preview !== null
            ? `Buys ${formatUnits(fromBaseUnits(preview))} units at ${formatUsdt(nav?.nav)} USDT per unit.`
            : "Units are priced at the current NAV; the unit count is fixed at purchase and does not grow."}
      </p>
    </div>
  );
}

function RedeemPanel({ service, position, nav, onDone }: { service: string; position: Position; nav: FundNav | null; onDone: () => void }) {
  const [units, setUnits] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<Redemption | null>(null);

  const estimate = cashForUnits(units, nav?.nav);
  const overdraw = toBaseUnits(units) > toBaseUnits(position.units);

  const submit = async () => {
    setSubmitting(true);
    setError(null);
    setDone(null);
    try {
      const redemption = await submitRedeem({ service, units });
      setDone(redemption);
      setUnits("");
      onDone();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="space-y-3 rounded-lg border border-border bg-main-surface p-4">
      {done && (
        <Alert>
          <Clock className="size-4 text-main-accent-t3" />
          <AlertTitle>{done.state === "completed" ? "Redemption completed" : "Redemption queued"}</AlertTitle>
          <AlertDescription>
            {done.state === "completed"
              ? `${formatUnits(done.units)} units redeemed for ${formatUsdt(done.cash)} USDT at ${formatUsdt(done.nav)} USDT NAV.`
              : `${formatUnits(done.units)} units reserved — queued until the fund tops up, then priced at the settle NAV.`}
          </AlertDescription>
        </Alert>
      )}
      {error && (
        <Alert variant="destructive">
          <TriangleAlert className="size-4" />
          <AlertTitle>Redemption failed</AlertTitle>
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}

      {/* The one genuinely surprising rule of this product, stated on the action itself. */}
      <p className="flex items-start gap-1.5 text-xs text-main-accent-t3">
        <Clock className="mt-0.5 size-3.5 shrink-0" />
        <span>
          Your units are reserved the moment you submit, but the cash is priced when the redemption settles — not now. A figure shown here is an estimate at today&apos;s NAV.
          <TipAnchor anchor="invest.redeem.queue" />
        </span>
      </p>

      <div className="flex flex-wrap items-end gap-3">
        <label className="flex min-w-48 flex-1 flex-col gap-1.5">
          <span className="flex items-center justify-between text-sm">
            <span className="flex items-center gap-1.5">
              Units to redeem
              <TipAnchor anchor="invest.redeem.units" />
            </span>
            <button type="button" className="text-xs text-main-accent-t1 hover:underline" onClick={() => setUnits(position.units ?? "0")}>
              Max
            </button>
          </span>
          <Input value={units} onChange={(e) => setUnits(e.target.value)} inputMode="decimal" placeholder="0.00" className="w-full" />
        </label>
        <Button type="button" variant="outline" disabled={submitting || estimate === null || overdraw} onClick={submit}>
          {submitting ? <Loader2 className="size-4 animate-spin" /> : <ArrowDownToLine className="size-4" />}
          Redeem
        </Button>
      </div>

      <p className={cn("text-xs", overdraw ? "text-destructive" : "text-muted-foreground")}>
        {overdraw
          ? `You hold ${formatUnits(position.units)} units.`
          : estimate !== null
            ? `≈ ${formatUsdt(fromBaseUnits(estimate))} USDT at today's NAV — the settle price may differ.`
            : `${formatUnits(position.units)} units held.`}
      </p>
    </div>
  );
}

/** Queued redemptions for this product, inline — they belong to the fund they came from,
 *  not to a separate activity list the holder has to go and find. */
function QueuedList({ items, onDone }: { items: Redemption[]; onDone: () => void }) {
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const cancel = async (id: string) => {
    setBusy(id);
    setError(null);
    try {
      await cancelRedemption(id);
      onDone();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="space-y-2 rounded-lg border border-main-accent-t3/30 bg-main-accent-t3/5 p-3">
      <p className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-wide text-main-accent-t3">
        Awaiting settlement
        <TipAnchor anchor="invest.activity.status" />
      </p>
      {error && <p className="text-xs text-destructive">{error}</p>}
      {items.map((r) => (
        <div key={r.id ?? ""} className="flex items-center justify-between gap-3 text-sm">
          <span>
            <span className="font-medium">{formatUnits(r.units)} units</span> <span className="text-muted-foreground">reserved, priced at settle</span>
          </span>
          <span className="flex items-center gap-2">
            <Button type="button" variant="outline" size="sm" disabled={busy === (r.id ?? "")} onClick={() => cancel(r.id ?? "")}>
              {busy === (r.id ?? "") ? <Loader2 className="size-3 animate-spin" /> : <X className="size-3" />}
              Cancel
            </Button>
            <TipAnchor anchor="invest.activity.cancel" />
          </span>
        </div>
      ))}
    </div>
  );
}

function Note({ tone, children }: { tone: "amber" | "muted"; children: React.ReactNode }) {
  return (
    <p
      className={cn(
        "rounded-lg border px-3 py-2 text-xs",
        tone === "amber" ? "border-main-accent-t3/30 bg-main-accent-t3/5 text-main-accent-t3" : "border-border bg-foreground/5 text-muted-foreground",
      )}
    >
      {children}
    </p>
  );
}

function Stat({ label, value, emphasis, tip, tone, icon }: { label: string; value: string; emphasis?: boolean; tip?: Parameters<typeof TipAnchor>[0]["anchor"]; tone?: string; icon?: React.ReactNode }) {
  return (
    <div className="rounded-lg border border-border bg-main-surface p-3">
      <div className="flex items-center gap-1.5">
        <p className="text-xs uppercase tracking-wide text-muted-foreground">{label}</p>
        {tip && <TipAnchor anchor={tip} />}
      </div>
      <p className={cn("flex items-center gap-1 tabular-nums", emphasis ? "text-xl font-semibold" : "text-base", tone)}>
        {icon}
        {value}
      </p>
    </div>
  );
}
