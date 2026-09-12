"use client";

// A payment consilium's terms, as the owners' room shows them beside the tally: the two
// ends, the tier, and the initiator's reason. An external destination shows its address
// whole — the same rule as the approval email and the approval page, so an owner who
// checks it here and approves it there is looking at the same characters (policy 13).

import { useT } from "@evinvest/i18n/react";

import { tierLabel } from "@/entities/payment/lib/format";
import { PaymentEndSummary } from "@/entities/payment/ui/payment-end";
import type { ConsiliumPaymentTerms } from "@/shared/contracts/payments";

export function PaymentTerms({ terms }: { terms: ConsiliumPaymentTerms }) {
  const t = useT();
  const external = terms.destination.kind === "external";
  return (
    <div className="flex flex-col gap-3">
      <div className="grid gap-3 sm:grid-cols-2">
        <div className="flex flex-col gap-1">
          <span className="text-xs font-medium text-muted-foreground">{t("consilium.payment.from")}</span>
          <PaymentEndSummary end={terms.source} className="text-sm" />
        </div>
        <div className="flex flex-col gap-1">
          <span className="text-xs font-medium text-muted-foreground">{t("consilium.payment.to")}</span>
          {external ? (
            <span className="text-sm font-medium text-foreground">{terms.destination.label || "—"}</span>
          ) : (
            <PaymentEndSummary end={terms.destination} className="text-sm" />
          )}
        </div>
      </div>
      {external && (
        <p className="break-all rounded-lg border border-border bg-main-surface px-3 py-2.5 font-mono-tech text-xs leading-relaxed text-foreground">
          {terms.destination.address || "—"}
        </p>
      )}
      <div className="flex flex-col gap-1">
        <span className="text-xs font-medium text-muted-foreground">{t("consilium.payment.reason")}</span>
        {/* The initiator's words, whole, set apart from the room's own. */}
        <blockquote className="whitespace-pre-line border-l-2 border-main-accent-t3/60 pl-3 text-sm leading-relaxed text-foreground">{terms.reason?.trim() || "—"}</blockquote>
      </div>
      <span className="text-xs text-muted-foreground">{t("consilium.payment.tier", { tier: tierLabel(terms.tier, t) })}</span>
    </div>
  );
}

/** The one-line name of a settled payment in the room's history: "Fund revenue → Alpha fund". */
export function paymentWords(terms: ConsiliumPaymentTerms): string {
  return `${terms.source.label || "—"} → ${terms.destination.label || "—"}`;
}
