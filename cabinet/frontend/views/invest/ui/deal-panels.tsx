"use client";

// The redeem form and the queue that follows a redemption. Extracted from the invest list
// when the product page arrived: a deal has to behave identically wherever it is initiated
// from, and the surest way to guarantee that is one implementation. The subscribe form is
// `./subscribe-panel` — it grew a balance, a tier gate and a top-up of its own (#396).
//
// Neither takes an `onDone`. The mutations they call name what they moved (see
// `entities/fund/model/fund-resource.ts`), so the page around them — and Home, and Wallet,
// and anything else showing a figure a deal touched — refreshes itself. A callback per
// panel was the same job done once per call site, which is the version that goes stale.

import { useLocale, useT } from "@evinvest/i18n/react";
import { ArrowDownToLine, Clock, TriangleAlert, X } from "lucide-react";
import { useState } from "react";

import { Alert, AlertDescription, AlertTitle, Button, Input, Spinner } from "@evinvest/uikit";

import { cancelRedemption, submitRedeem } from "@/entities/fund/model/fund-resource";
import type { FundNav, Position, Redemption } from "@/shared/contracts";
import { errorMessage } from "@/shared/lib/api-client";
import { cn } from "@/shared/lib/cn";
import { TipAnchor } from "@/shared/tips";
import { Panel, PanelPresence } from "@/shared/ui/motion";
import { SectionLabel } from "@/shared/ui/page-frame";
import { formatUnits, formatUsdt, fromBaseUnits, toBaseUnits } from "@/views/invest/lib/format";
import { cashForUnits } from "@/views/invest/lib/product";
import { TradeLink } from "@/views/invest/ui/trade-link";

/** `inKind`: the units are not backed by fund cash, so the hub refuses the redeem (412) —
 *  the form says so before the click, and offers the book instead. */
export function RedeemPanel({ service, position, nav, inKind = false }: { service: string; position: Position; nav: FundNav | null; inKind?: boolean }) {
  const t = useT();
  const locale = useLocale();
  const [units, setUnits] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<unknown>(null);
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
    } catch (e) {
      // The error itself: `errorMessage` resolves its `code` in the reader's locale.
      setError(e);
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="space-y-3 rounded-lg border border-border bg-secondary p-4">
      <PanelPresence>
        {done && (
          <Panel key="receipt" from="bottom">
            <Alert>
              <Clock className="size-4 text-accent-warn" />
              <AlertTitle>{t(done.state === "completed" ? "invest.redeemCompleted" : "invest.redeemQueued")}</AlertTitle>
              <AlertDescription>
                {done.state === "completed"
                  ? t("invest.redeemReceiptCompleted", { n: Number(done.units ?? 0), units: formatUnits(done.units, locale), cash: formatUsdt(done.cash, locale), nav: formatUsdt(done.nav, locale) })
                  : t("invest.redeemReceiptQueued", { n: Number(done.units ?? 0), units: formatUnits(done.units, locale) })}
              </AlertDescription>
            </Alert>
          </Panel>
        )}
        {!!error && (
          <Panel key="error" from="bottom">
            <Alert variant="destructive">
              <TriangleAlert className="size-4" />
              <AlertTitle>{t("invest.redeemFailed")}</AlertTitle>
              <AlertDescription>{errorMessage(error, t)}</AlertDescription>
            </Alert>
          </Panel>
        )}
      </PanelPresence>

      {/* The one genuinely surprising rule of this product, stated on the action itself —
          unless the action is refused outright, when the refusal is the rule. */}
      {!inKind && (
        <p className="flex items-start gap-1.5 text-xs text-accent-warn">
          <Clock className="mt-0.5 size-3.5 shrink-0" />
          <span>
            {t("invest.redeemTimingNote")}
            <TipAnchor anchor="invest.redeem.queue" />
          </span>
        </p>
      )}

      <div className="flex flex-wrap items-end gap-3">
        <label className="flex min-w-48 flex-1 flex-col gap-1.5">
          <span className="flex items-center justify-between text-sm">
            <span className="flex items-center gap-1.5">
              {t("invest.unitsToRedeem")}
              <TipAnchor anchor="invest.redeem.units" />
            </span>
            <button type="button" className="text-xs text-primary-ink hover:underline" onClick={() => setUnits(position.units ?? "0")}>
              {t("ui.max")}
            </button>
          </span>
          <Input value={units} onChange={(e) => setUnits(e.target.value)} inputMode="decimal" placeholder="0.00" className="w-full" />
        </label>
        <Button type="button" variant="outline" disabled={inKind || submitting || estimate === null || overdraw} onClick={submit}>
          {submitting ? <Spinner aria-hidden /> : <ArrowDownToLine className="size-4" />}
          {t("invest.redeem")}
        </Button>
      </div>

      {inKind && (
        <div className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-accent-warn/30 bg-accent-warn/5 px-3 py-2">
          <p className="text-xs text-accent-warn">{t("invest.redeemUnbacked")}</p>
          {/* Renders only while the book is open — a closed book has no way out to offer. */}
          <TradeLink service={service} />
        </div>
      )}

      <p className={cn("text-xs", overdraw ? "text-accent-error" : "text-ink-soft")}>
        {overdraw
          ? t("invest.youHoldUnits", { n: Number(position.units ?? 0), units: formatUnits(position.units, locale) })
          : estimate !== null
            ? t("invest.redeemEstimate", { amount: formatUsdt(fromBaseUnits(estimate), locale) })
            : t("invest.unitsHeld", { n: Number(position.units ?? 0), units: formatUnits(position.units, locale) })}
      </p>
    </div>
  );
}

/** Queued redemptions for this product, inline — they belong to the fund they came from,
 *  not to a separate activity list the holder has to go and find. */
export function QueuedList({ items }: { items: Redemption[] }) {
  const t = useT();
  const locale = useLocale();
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<unknown>(null);

  const cancel = async (id: string) => {
    setBusy(id);
    setError(null);
    try {
      await cancelRedemption(id);
    } catch (e) {
      // The error itself: `errorMessage` resolves its `code` in the reader's locale.
      setError(e);
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="space-y-2 rounded-lg border border-accent-warn/30 bg-accent-warn/5 p-3">
      <SectionLabel className="flex items-center gap-1.5 text-accent-warn">
        {t("invest.awaitingSettlement")}
        <TipAnchor anchor="invest.activity.status" />
      </SectionLabel>
      {!!error && <p className="text-xs text-accent-error">{errorMessage(error, t)}</p>}
      {items.map((r) => (
        <div key={r.id ?? ""} className="flex items-center justify-between gap-3 text-sm">
          <span>
            <span className="font-medium">{t("dash.unitsAmount", { n: Number(r.units ?? 0), units: formatUnits(r.units, locale) })}</span>{" "}
            <span className="text-ink-soft">{t("invest.reservedPricedAtSettle")}</span>
          </span>
          <span className="flex items-center gap-2">
            <Button type="button" variant="outline" size="sm" disabled={busy === (r.id ?? "")} onClick={() => cancel(r.id ?? "")}>
              {busy === (r.id ?? "") ? <Spinner className="size-3" aria-hidden /> : <X className="size-3" />}
              {t("ui.cancel")}
            </Button>
            <TipAnchor anchor="invest.activity.cancel" />
          </span>
        </div>
      ))}
    </div>
  );
}
