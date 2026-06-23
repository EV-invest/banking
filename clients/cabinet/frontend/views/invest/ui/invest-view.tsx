"use client";

import { ArrowDownToLine, Clock, Loader2, Sparkles, TrendingUp, TriangleAlert, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { Alert, AlertDescription, AlertTitle, Badge, Button, Card, CardContent, Input, Skeleton, Tabs, TabsContent, TabsList, TabsTrigger } from "@evinvest/uikit";

import { cancelRedemption, fetchFundNav, fetchPositions, fetchRedemptions, submitRedeem, submitSubscribe } from "@/entities/fund/api/fund-client";
import type { FundNav, Position, Redemption } from "@/shared/contracts";
import { cn } from "@/shared/lib/cn";
import { formatSignedUsdt, formatUnits, formatUsdt, isNegative, isZero } from "@/views/invest/lib/format";

const TEAL_CTA = "bg-main-accent-t1 text-main-black hover:bg-main-accent-t1/90";

// The demo fund — the cabinet ships one service surface; the form lets you point at
// another fund id if needed, defaulting here.
const DEFAULT_SERVICE = "fund";

export function InvestView() {
  const [positions, setPositions] = useState<Position[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    fetchPositions()
      .then((list) => {
        setPositions(list.positions ?? []);
        setError(null);
      })
      .catch((e: Error) => setError(e.message));
  }, []);

  useEffect(load, [load]);

  return (
    <div className="container max-w-5xl space-y-8 py-12">
      <header className="space-y-1">
        <p className="font-mono-tech text-xs uppercase tracking-widest text-main-accent-t1">Invest</p>
        <h1 className="text-3xl font-semibold">Your fund shares</h1>
        <p className="text-sm text-muted-foreground">Subscribe USDT for units — your value tracks the fund&apos;s NAV, not the unit count.</p>
      </header>

      {error && (
        <Alert variant="destructive">
          <TriangleAlert className="size-4" />
          <AlertTitle>Couldn&apos;t load your positions</AlertTitle>
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}

      <PositionsList positions={positions} />

      <Tabs defaultValue="subscribe">
        <TabsList>
          <TabsTrigger value="subscribe">Subscribe</TabsTrigger>
          <TabsTrigger value="redeem">Redeem</TabsTrigger>
          <TabsTrigger value="activity">Activity</TabsTrigger>
        </TabsList>

        <TabsContent value="subscribe" className="pt-6">
          <SubscribePanel onDone={load} />
        </TabsContent>
        <TabsContent value="redeem" className="pt-6">
          <RedeemPanel positions={positions} onDone={load} />
        </TabsContent>
        <TabsContent value="activity" className="pt-6">
          <ActivityPanel />
        </TabsContent>
      </Tabs>
    </div>
  );
}

// One position card: units, current NAV (+ stale badge), value (units × NAV), and P&L.
function PositionCard({ position }: { position: Position }) {
  const [nav, setNav] = useState<FundNav | null>(null);
  const service = position.service ?? DEFAULT_SERVICE;

  useEffect(() => {
    let active = true;
    fetchFundNav(service)
      .then((n) => active && setNav(n))
      .catch(() => active && setNav(null));
    return () => {
      active = false;
    };
  }, [service]);

  const loss = isNegative(position.pnl);
  const flat = isZero(position.pnl);
  const pnlColor = loss && !flat ? "text-main-accent-t4" : "text-main-accent-t2";

  return (
    <Card>
      <CardContent className="space-y-4 pt-6">
        <div className="flex items-center justify-between gap-2">
          <p className="font-medium">{service}</p>
          {nav?.stale && (
            <Badge variant="outline" className="gap-1 border-main-accent-t3/40 text-main-accent-t3">
              <Clock className="size-3" /> Stale NAV
            </Badge>
          )}
        </div>
        <div className="grid gap-4 sm:grid-cols-3">
          <Stat label="Units" value={`${formatUnits(position.units)}`} />
          <Stat label="NAV" value={`${formatUsdt(position.nav)} USDT`} />
          <Stat label="Value" value={`${formatUsdt(position.value)} USDT`} emphasis />
        </div>
        <div className="flex items-center justify-between rounded-lg border border-border bg-main-surface p-3 text-sm">
          <span className="text-muted-foreground">Profit &amp; loss</span>
          <span className={cn("flex items-center gap-1 font-semibold tabular-nums", pnlColor)}>
            <TrendingUp className={cn("size-4", loss && !flat && "rotate-180")} />
            {formatSignedUsdt(position.pnl)} USDT
          </span>
        </div>
      </CardContent>
    </Card>
  );
}

function PositionsList({ positions }: { positions: Position[] | null }) {
  if (!positions) return <Skeleton className="h-44 w-full" />;
  if (positions.length === 0) {
    return (
      <Card>
        <CardContent className="flex flex-col items-center gap-2 py-12 text-center text-muted-foreground">
          <Sparkles className="size-6" />
          <p className="text-sm">No fund positions yet — subscribe below to buy your first units.</p>
        </CardContent>
      </Card>
    );
  }
  return (
    <div className="grid gap-4 md:grid-cols-2">
      {positions.map((p) => (
        <PositionCard key={p.service ?? ""} position={p} />
      ))}
    </div>
  );
}

function Stat({ label, value, emphasis }: { label: string; value: string; emphasis?: boolean }) {
  return (
    <div className="rounded-lg border border-border bg-main-surface p-4">
      <p className="text-xs uppercase tracking-wide text-muted-foreground">{label}</p>
      <p className={cn("tabular-nums", emphasis ? "text-2xl font-semibold" : "text-lg")}>{value}</p>
    </div>
  );
}

function SubscribePanel({ onDone }: { onDone: () => void }) {
  const [service, setService] = useState(DEFAULT_SERVICE);
  const [amount, setAmount] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<{ units?: string; nav?: string } | null>(null);

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

  const valid = service.trim().length > 0 && Number(amount) > 0;

  return (
    <div className="max-w-xl space-y-5">
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

      <Card>
        <CardContent className="space-y-4 pt-6">
          <label className="block space-y-1.5">
            <span className="text-sm">Fund</span>
            <Input value={service} onChange={(e) => setService(e.target.value)} placeholder="fund id" spellCheck={false} />
          </label>

          <label className="block space-y-1.5">
            <span className="text-sm">Amount</span>
            <Input value={amount} onChange={(e) => setAmount(e.target.value)} inputMode="decimal" placeholder="0.00" />
          </label>

          <p className="text-xs text-muted-foreground">Units are priced at the current NAV. Profit comes from the NAV rising, not from extra units.</p>

          <Button type="button" className={cn("w-full", TEAL_CTA)} disabled={!valid || submitting} onClick={submit}>
            {submitting ? <Loader2 className="size-4 animate-spin" /> : <Sparkles className="size-4" />}
            Subscribe
          </Button>
        </CardContent>
      </Card>
    </div>
  );
}

function RedeemPanel({ positions, onDone }: { positions: Position[] | null; onDone: () => void }) {
  if (!positions) return <Skeleton className="h-40 w-full" />;
  if (positions.length === 0) {
    return (
      <Card>
        <CardContent className="flex flex-col items-center gap-2 py-12 text-center text-muted-foreground">
          <Sparkles className="size-6" />
          <p className="text-sm">Nothing to redeem — subscribe to a fund first.</p>
        </CardContent>
      </Card>
    );
  }
  return (
    <div className="space-y-4">
      {positions.map((p) => (
        <RedeemRow key={p.service ?? ""} position={p} onDone={onDone} />
      ))}
    </div>
  );
}

function RedeemRow({ position, onDone }: { position: Position; onDone: () => void }) {
  const service = position.service ?? DEFAULT_SERVICE;
  const [units, setUnits] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<Redemption | null>(null);

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

  const valid = Number(units) > 0;

  return (
    <Card>
      <CardContent className="space-y-4 pt-6">
        <div className="flex items-center justify-between gap-2">
          <p className="font-medium">{service}</p>
          <span className="text-xs text-muted-foreground">{formatUnits(position.units)} units held</span>
        </div>

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

        <div className="flex items-end gap-2">
          <label className="block flex-1 space-y-1.5">
            <span className="flex items-center justify-between text-sm">
              <span>Units to redeem</span>
              <button type="button" className="text-xs text-main-accent-t1 hover:underline" onClick={() => setUnits(position.units ?? "0")}>
                Max
              </button>
            </span>
            <Input value={units} onChange={(e) => setUnits(e.target.value)} inputMode="decimal" placeholder="0.00" />
          </label>
          <Button type="button" variant="outline" disabled={!valid || submitting} onClick={submit}>
            {submitting ? <Loader2 className="size-4 animate-spin" /> : <ArrowDownToLine className="size-4" />}
            Redeem
          </Button>
        </div>
        <p className="text-xs text-muted-foreground">Redemptions are accept-and-queue — cash is priced at the settle-time NAV.</p>
      </CardContent>
    </Card>
  );
}

const STATUS_STYLES: Record<string, string> = {
  queued: "bg-main-accent-t3/15 text-main-accent-t3",
  completed: "bg-main-accent-t2/15 text-main-accent-t2",
  failed: "bg-main-accent-t4/15 text-main-accent-t4",
  cancelled: "bg-muted text-muted-foreground",
};

function StatusPill({ state }: { state: string | undefined }) {
  const key = state ?? "queued";
  return <span className={cn("rounded-full px-2.5 py-0.5 text-xs font-medium capitalize", STATUS_STYLES[key] ?? "bg-muted text-muted-foreground")}>{key}</span>;
}

function ActivityPanel() {
  const [items, setItems] = useState<Redemption[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const load = useCallback(() => {
    fetchRedemptions()
      .then((list) => setItems(list.redemptions ?? []))
      .catch((e: Error) => setError(e.message));
  }, []);

  useEffect(load, [load]);

  const cancel = async (id: string) => {
    setBusy(id);
    try {
      await cancelRedemption(id);
      load();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(null);
    }
  };

  if (error) return <p className="text-sm text-destructive">{error}</p>;
  if (!items) return <Skeleton className="h-40 w-full" />;
  if (items.length === 0) {
    return (
      <Card>
        <CardContent className="flex flex-col items-center gap-2 py-12 text-center text-muted-foreground">
          <ArrowDownToLine className="size-6" />
          <p className="text-sm">No redemptions yet.</p>
        </CardContent>
      </Card>
    );
  }
  return (
    <Card>
      <CardContent className="divide-y divide-border p-0">
        {items.map((r) => {
          const id = r.id ?? "";
          return (
            <div key={id} className="flex items-center justify-between gap-4 px-4 py-3">
              <div className="min-w-0 space-y-0.5">
                <p className="text-sm">
                  <span className="font-medium">{formatUnits(r.units)} units</span>{" "}
                  <span className="text-muted-foreground">{r.cash ? `→ ${formatUsdt(r.cash)} USDT` : "from " + (r.service ?? "fund")}</span>
                </p>
                <p className="font-mono-tech text-xs text-muted-foreground">{r.service ?? "fund"}</p>
              </div>
              <div className="flex shrink-0 items-center gap-2">
                <StatusPill state={r.state} />
                {r.state === "queued" && (
                  <Button type="button" variant="outline" size="sm" disabled={busy === id} onClick={() => cancel(id)}>
                    {busy === id ? <Loader2 className="size-3 animate-spin" /> : <X className="size-3" />}
                    Cancel
                  </Button>
                )}
              </div>
            </div>
          );
        })}
      </CardContent>
    </Card>
  );
}
