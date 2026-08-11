"use client";

// The two deal forms and the queue that follows a redemption. Extracted from the invest
// list when the product page arrived: a subscription has to behave identically wherever
// it is initiated from, and the surest way to guarantee that is one implementation.

import { ArrowDownToLine, Clock, Loader2, Sparkles, TriangleAlert, X } from "lucide-react";
import { useState } from "react";

import { Alert, AlertDescription, AlertTitle, Button, Input } from "@evinvest/uikit";

import { cancelRedemption, submitRedeem, submitSubscribe } from "@/entities/fund/api/fund-client";
import type { FundNav, Position, Redemption } from "@/shared/contracts";
import { cn } from "@/shared/lib/cn";
import { TipAnchor } from "@/shared/tips";
import { Panel, PanelPresence } from "@/shared/ui/motion";
import { formatUnits, formatUsdt, fromBaseUnits, toBaseUnits } from "@/views/invest/lib/format";
import { cashForUnits, unitsForCash } from "@/views/invest/lib/product";
import { TEAL_CTA } from "@/views/invest/ui/atoms";

export function SubscribePanel({ service, nav, onDone }: { service: string; nav: FundNav | null; onDone: () => void }) {
  const [amount, setAmount] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<{ units?: string; nav?: string } | null>(null);

  // Exact preview, floored the same way the hub floors it — so "0 units" is visible here
  // instead of arriving as a rejection.
  const preview = unitsForCash(amount, nav?.nav);
  const dust = toBaseUnits(amount) > 0n && preview === 0n;
  // The supply cap, checked before the submit rather than after it. `remaining_capacity`
  // already counts in-flight mints, so this is the same figure the hub will gate on.
  const headroom = nav ? toBaseUnits(nav.remaining_capacity) : null;
  const overCap = preview !== null && headroom !== null && preview > headroom;

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
      {/* The receipt and the failure occupy the same slot and replace one another, so
          they share a presence boundary: retrying after an error swaps the panel in
          place instead of collapsing the form and re-expanding it. */}
      <PanelPresence>
        {done && (
          <Panel key="receipt" from="bottom">
            <Alert>
              <Sparkles className="size-4 text-main-accent-t2" />
              <AlertTitle>Subscription received</AlertTitle>
              <AlertDescription>
                Minted {formatUnits(done.units)} units at {formatUsdt(done.nav)} USDT NAV — your position updates shortly.
              </AlertDescription>
            </Alert>
          </Panel>
        )}
        {error && (
          <Panel key="error" from="bottom">
            <Alert variant="destructive">
              <TriangleAlert className="size-4" />
              <AlertTitle>Subscription failed</AlertTitle>
              <AlertDescription>{error}</AlertDescription>
            </Alert>
          </Panel>
        )}
      </PanelPresence>

      <div className="flex flex-wrap items-end gap-3">
        <label className="flex min-w-48 flex-1 flex-col gap-1.5">
          <span className="flex items-center gap-1.5 text-sm">
            Amount (USDT)
            <TipAnchor anchor="invest.subscribe.amount" />
          </span>
          <Input value={amount} onChange={(e) => setAmount(e.target.value)} inputMode="decimal" placeholder="0.00" className="w-full" />
        </label>
        <Button type="button" className={cn(TEAL_CTA)} disabled={submitting || preview === null || preview === 0n || overCap} onClick={submit}>
          {submitting ? <Loader2 className="size-4 animate-spin" /> : <Sparkles className="size-4" />}
          Subscribe
        </Button>
      </div>

      <p className={cn("text-xs", dust || overCap ? "text-destructive" : "text-muted-foreground")}>
        {dust
          ? `Too small to buy a whole unit at ${formatUsdt(nav?.nav)} — increase the amount.`
          : overCap
            ? `Only ${formatUnits(fromBaseUnits(headroom ?? 0n))} units are left before this fund reaches its cap — reduce the amount.`
            : preview !== null
              ? `Buys ${formatUnits(fromBaseUnits(preview))} units at ${formatUsdt(nav?.nav)} USDT per unit.`
              : "Units are priced at the current NAV; the unit count is fixed at purchase and does not grow."}
      </p>
    </div>
  );
}

export function RedeemPanel({ service, position, nav, onDone }: { service: string; position: Position; nav: FundNav | null; onDone: () => void }) {
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
      <PanelPresence>
        {done && (
          <Panel key="receipt" from="bottom">
            <Alert>
              <Clock className="size-4 text-main-accent-t3" />
              <AlertTitle>{done.state === "completed" ? "Redemption completed" : "Redemption queued"}</AlertTitle>
              <AlertDescription>
                {done.state === "completed"
                  ? `${formatUnits(done.units)} units redeemed for ${formatUsdt(done.cash)} USDT at ${formatUsdt(done.nav)} USDT NAV.`
                  : `${formatUnits(done.units)} units reserved — queued until the fund tops up, then priced at the settle NAV.`}
              </AlertDescription>
            </Alert>
          </Panel>
        )}
        {error && (
          <Panel key="error" from="bottom">
            <Alert variant="destructive">
              <TriangleAlert className="size-4" />
              <AlertTitle>Redemption failed</AlertTitle>
              <AlertDescription>{error}</AlertDescription>
            </Alert>
          </Panel>
        )}
      </PanelPresence>

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
export function QueuedList({ items, onDone }: { items: Redemption[]; onDone: () => void }) {
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
