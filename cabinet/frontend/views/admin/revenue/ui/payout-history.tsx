"use client";

// Every payout the fund made of its own earnings — HISTORY since #245: nothing opens a
// revenue payout any more, and the rows here are the ones queued before the kind retired.
// A still-queued one can be cancelled; the rest are the record.

import { Banknote } from "lucide-react";

import { useLocale, useT } from "@evinvest/i18n/react";
import { Button, Card, CardContent, Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle, Skeleton, Spinner, Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@evinvest/uikit";

import type { RevenuePayout } from "@/shared/contracts/admin";
import { cn } from "@/shared/lib/cn";
import { shortAddress } from "@/shared/lib/money";
import { networkLabel } from "@/shared/lib/rail";
import { NetworkMark } from "@/shared/ui/icons/networks";
import { Settled } from "@/shared/ui/motion";
import { ResourceError } from "@/shared/ui/resource-error";
import { formatUsdt, stateLabel } from "@/views/admin/lib/format";
import { EDGE_CELL, TABLE_HEAD } from "@/views/admin/lib/table";

/** In flight — the operator can still act on these; the rest are history. */
const OPEN_STATES = new Set(["queued", "processing"]);

export function PayoutHistory({
  history,
  error,
  onRetry,
  busy,
  onCancel,
}: {
  history: RevenuePayout[] | null;
  /** The read failed and nothing is on screen — not the same as an empty history. */
  error: unknown;
  onRetry: () => void;
  busy: string | null;
  onCancel: (id: string) => void;
}) {
  const t = useT();
  if (!history && error !== null && error !== undefined) return <ResourceError error={error} onRetry={onRetry} />;
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
            // Five columns do not fit a phone: the kit's wrapper scrolls the table inside its box.
            <Table className="min-w-140">
              <TableHeader>
                {/* i18n-max: 14 per header — a long header widens the scroll, not a cell. */}
                <TableRow>
                  <TableHead className={cn(TABLE_HEAD, EDGE_CELL)}>{t("ui.destination")}</TableHead>
                  <TableHead className={cn(TABLE_HEAD, EDGE_CELL)}>{t("admin.payments.col.amountUsdt")}</TableHead>
                  <TableHead className={cn(TABLE_HEAD, EDGE_CELL)}>{t("admin.col.state")}</TableHead>
                  <TableHead className={cn(TABLE_HEAD, EDGE_CELL)}>{t("admin.col.transaction")}</TableHead>
                  <TableHead className={cn(TABLE_HEAD, EDGE_CELL, "text-right")}>{t("admin.col.actions")}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {history.map((payout) => (
                  <PayoutRow key={payout.id} payout={payout} busy={busy === payout.id} onCancel={() => onCancel(payout.id)} />
                ))}
              </TableBody>
            </Table>
          )}
        </Settled>
      </CardContent>
    </Card>
  );
}

function PayoutRow({ payout, busy, onCancel }: { payout: RevenuePayout; busy: boolean; onCancel: () => void }) {
  const t = useT();
  const locale = useLocale();
  const open = OPEN_STATES.has(payout.state);
  return (
    <TableRow>
      <TableCell className={EDGE_CELL}>
        <p className="flex items-center gap-1.5 text-xs text-ink-soft">
          <NetworkMark network={payout.network} className="size-3.5 shrink-0" />
          {networkLabel(payout.network)}
        </p>
        <p className="font-mono-tech text-xs" title={payout.address}>
          {shortAddress(payout.address)}
        </p>
      </TableCell>
      <TableCell className={cn(EDGE_CELL, "tabular-nums")}>{formatUsdt(payout.amount, locale)}</TableCell>
      <TableCell className={EDGE_CELL}>
        <span className={stateTone(payout.state)}>{stateLabel(payout.state, t)}</span>
      </TableCell>
      <TableCell className={cn(EDGE_CELL, "font-mono-tech text-xs text-ink-soft")} title={payout.tx_ref || undefined}>
        {payout.tx_ref ? shortAddress(payout.tx_ref) : "—"}
      </TableCell>
      <TableCell className={EDGE_CELL}>
        <div className="flex justify-end">
          {payout.state === "queued" ? (
            <Button type="button" variant="outline" size="sm" disabled={busy} aria-busy={busy} onClick={onCancel}>
              {busy ? <Spinner aria-hidden /> : null}
              {t("ui.cancel")}
            </Button>
          ) : (
            <span className="text-xs text-ink-soft">{open ? t("admin.revenue.inFlight") : "—"}</span>
          )}
        </div>
      </TableCell>
    </TableRow>
  );
}

function stateTone(state: string): string {
  switch (state) {
    case "completed":
      return "text-positive";
    case "queued":
    case "processing":
      return "text-accent-warn";
    case "failed":
      return "text-accent-error";
    default:
      return "text-ink-soft";
  }
}
