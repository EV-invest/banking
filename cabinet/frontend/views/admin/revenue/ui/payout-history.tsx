"use client";

// Every payout the fund made of its own earnings. Unchanged from the days the proposal
// form sat above it: a consilium that carries executes as an ordinary withdrawal, and
// that is where it appears — whether it was opened as a revenue payout or as an external
// payment order from the revenue claim.

import { Banknote, Loader2 } from "lucide-react";

import { useT } from "@evinvest/i18n/react";
import { Button, Card, CardContent, Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle, Skeleton } from "@evinvest/uikit";

import type { RevenuePayout } from "@/shared/contracts/admin";
import { shortAddress } from "@/shared/lib/money";
import { networkLabel } from "@/shared/lib/rail";
import { NetworkMark } from "@/shared/ui/icons/networks";
import { Settled } from "@/shared/ui/motion";
import { formatUsd, stateLabel } from "@/views/admin/lib/format";

/** In flight — the operator can still act on these; the rest are history. */
const OPEN_STATES = new Set(["queued", "processing"]);

export function PayoutHistory({ history, busy, onCancel }: { history: RevenuePayout[] | null; busy: string | null; onCancel: (id: string) => void }) {
  const t = useT();
  return (
    <Card>
      <CardContent className="p-0">
        <Settled loading={!history} skeleton={<Skeleton className="m-6 h-24" />}>
          {!history ? null : history.length === 0 ? (
            <div className="p-8">
              <Empty className="border md:p-6">
                <EmptyHeader>
                  <EmptyMedia variant="icon">
                    <Banknote />
                  </EmptyMedia>
                  <EmptyTitle>{t("admin.revenue.noPayouts")}</EmptyTitle>
                  <EmptyDescription>{t("admin.revenue.noPayoutsHintConsilium")}</EmptyDescription>
                </EmptyHeader>
              </Empty>
            </div>
          ) : (
            // Five columns do not fit a phone: the table scrolls inside its own box.
            <div className="overflow-x-auto">
              <table className="w-full min-w-140 text-sm">
                <thead>
                  {/* i18n-max: 14 per header — a long header widens the scroll, not a cell. */}
                  <tr className="border-b border-border text-left text-xs uppercase tracking-wide text-muted-foreground">
                    <th className="px-5 py-3 font-medium">{t("ui.destination")}</th>
                    <th className="px-5 py-3 font-medium">{t("ui.amount")}</th>
                    <th className="px-5 py-3 font-medium">{t("admin.col.state")}</th>
                    <th className="px-5 py-3 font-medium">{t("admin.col.transaction")}</th>
                    <th className="px-5 py-3 text-right font-medium">{t("admin.col.actions")}</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-border">
                  {history.map((payout) => (
                    <PayoutRow key={payout.id} payout={payout} busy={busy === payout.id} onCancel={() => onCancel(payout.id)} />
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </Settled>
      </CardContent>
    </Card>
  );
}

function PayoutRow({ payout, busy, onCancel }: { payout: RevenuePayout; busy: boolean; onCancel: () => void }) {
  const t = useT();
  const open = OPEN_STATES.has(payout.state);
  return (
    <tr>
      <td className="px-5 py-3">
        <p className="flex items-center gap-1.5 text-xs text-muted-foreground">
          <NetworkMark network={payout.network} className="size-3.5 shrink-0" />
          {networkLabel(payout.network)}
        </p>
        <p className="font-mono-tech text-xs" title={payout.address}>
          {shortAddress(payout.address)}
        </p>
      </td>
      <td className="px-5 py-3 tabular-nums">{formatUsd(payout.amount)}</td>
      <td className="px-5 py-3">
        <span className={stateTone(payout.state)}>{stateLabel(payout.state, t)}</span>
      </td>
      <td className="px-5 py-3 font-mono-tech text-xs text-muted-foreground" title={payout.tx_ref || undefined}>
        {payout.tx_ref ? shortAddress(payout.tx_ref) : "—"}
      </td>
      <td className="px-5 py-3">
        <div className="flex justify-end">
          {payout.state === "queued" ? (
            <Button type="button" variant="outline" size="sm" disabled={busy} onClick={onCancel}>
              {busy ? <Loader2 className="size-4 animate-spin" /> : null}
              {t("ui.cancel")}
            </Button>
          ) : (
            <span className="text-xs text-muted-foreground">{open ? t("admin.revenue.inFlight") : "—"}</span>
          )}
        </div>
      </td>
    </tr>
  );
}

function stateTone(state: string): string {
  switch (state) {
    case "completed":
      return "text-main-accent-t2";
    case "queued":
    case "processing":
      return "text-main-accent-t3";
    case "failed":
      return "text-destructive";
    default:
      return "text-muted-foreground";
  }
}
