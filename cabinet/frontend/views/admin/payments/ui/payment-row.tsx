"use client";

// One order in the list: who asked, from where to where, how much, and where the answer
// stands — a link into the owners' room for a consilium order, the seat's status for a
// consent one. The initiator alone may cancel, and only while the order is pending; the
// plane refuses anyone else, so the button is offered on every pending row and the
// refusal, if it comes, is shown rather than pre-empted.

import { ArrowDown } from "lucide-react";

import { useLocale, useT } from "@evinvest/i18n/react";
import { Button, InfoTip, InfoTipContent, InfoTipTrigger, Spinner, TableCell, TableRow } from "@evinvest/uikit";

import { consentLabel, consentTone, isPaymentOpen, paymentStateLabel, paymentStateTone, requirementLabel, tierLabel } from "@/entities/payment/lib/format";
import { PaymentEndSummary } from "@/entities/payment/ui/payment-end";
import type { Payment } from "@/shared/contracts/payments";
import { cn } from "@/shared/lib/cn";
import { expiresIn, formatMoment } from "@/shared/lib/datetime";
import { formatExactUsdt } from "@/shared/lib/money";
import { Link } from "@/shared/ui/cabinet-link";
import { EDGE_CELL } from "@/views/admin/lib/table";

export function PaymentRow({ payment, busy, onCancel }: { payment: Payment; busy: boolean; onCancel: () => void }) {
  const t = useT();
  const locale = useLocale();
  const open = isPaymentOpen(payment.state);
  // Top-aligned per cell rather than once on the row: the kit's cell sets `align-middle`
  // itself, and a class on the `tr` never wins against the cell's own.
  return (
    <TableRow>
      <TableCell className={cn(EDGE_CELL, "align-top")}>
        <p className="text-xs tabular-nums text-ink-soft">{formatMoment(payment.created_at, locale)}</p>
        <p className="truncate text-xs" title={payment.initiator_email}>
          {payment.initiator_email}
        </p>
      </TableCell>
      <TableCell className={cn(EDGE_CELL, "align-top")}>
        <div className="flex flex-col gap-1.5 text-sm">
          <PaymentEndSummary end={payment.source} />
          <ArrowDown aria-hidden className="size-3 text-ink-soft" />
          <PaymentEndSummary end={payment.destination} />
        </div>
      </TableCell>
      <TableCell className={cn(EDGE_CELL, "align-top text-sm font-medium tabular-nums")}>{formatExactUsdt(payment.amount, locale)}</TableCell>
      <TableCell className={cn(EDGE_CELL, "align-top text-xs")}>{tierLabel(payment.tier, t)}</TableCell>
      <TableCell className={cn(EDGE_CELL, "align-top text-xs")}>
        <p>{requirementLabel(payment.requirement, t)}</p>
        {payment.consilium_id ? (
          <Link href="/consilium" className="rounded-md text-primary-ink underline-offset-2 outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring">
            {t("admin.payments.openConsilium")}
          </Link>
        ) : payment.consent ? (
          <p className={cn("flex items-center gap-1", consentTone(payment.consent))}>
            {consentLabel(payment.consent, t)}
            {/* The plane's reason for voiding the seat, a tap away rather than in a `title`
                only a hovering mouse can read. */}
            {payment.consent.invalidation_reason && (
              <InfoTip>
                <InfoTipTrigger label={t("tips.a11y.about", { title: consentLabel(payment.consent, t) })} />
                <InfoTipContent className="text-ink-soft">{payment.consent.invalidation_reason}</InfoTipContent>
              </InfoTip>
            )}
          </p>
        ) : null}
      </TableCell>
      <TableCell className={cn(EDGE_CELL, "align-top whitespace-normal text-xs")}>
        <span className={cn("font-medium", paymentStateTone(payment.state))}>{paymentStateLabel(payment.state, t)}</span>
        {open && <p className="tabular-nums text-ink-soft">{expiresIn(payment.expires_at, t)}</p>}
        {payment.failure_reason && <p className="break-words text-accent-error">{payment.failure_reason}</p>}
      </TableCell>
      <TableCell className={cn(EDGE_CELL, "align-top")}>
        <div className="flex justify-end">
          {open ? (
            <Button type="button" variant="outline" size="sm" disabled={busy} aria-busy={busy} onClick={onCancel}>
              {busy ? <Spinner aria-hidden /> : null}
              {t("ui.cancel")}
            </Button>
          ) : (
            <span className="text-xs text-ink-soft">—</span>
          )}
        </div>
      </TableCell>
    </TableRow>
  );
}
