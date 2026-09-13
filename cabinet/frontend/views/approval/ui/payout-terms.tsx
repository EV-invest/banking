"use client";

// A revenue payout's terms, as the owners' approval page shows them: the exact amount and
// the whole address get the top of the card; everything past the separator is context.
// The sibling of `payment-terms.tsx` — a consilium carries one or the other.

import { useT } from "@evinvest/i18n/react";
import { Separator } from "@evinvest/uikit";

import type { RevenuePayout } from "@/shared/contracts/governance";
import { hashPrefix } from "@/shared/lib/hash";
import { formatExactUsdt } from "@/shared/lib/money";
import { networkLabel } from "@/shared/lib/rail";
import { DetailRow, FieldCaption, FullAddress } from "@/views/approval/ui/approval-chrome";

/** A revenue payout's terms: the exact amount and the whole address get the top of the card. */
export function PayoutTerms({ payout, payloadHash }: { payout: RevenuePayout | undefined; payloadHash: string }) {
  const t = useT();
  return (
    <>
      {/* The two things being agreed to get the whole top of the card: the exact amount
          and the whole address. Everything past the separator is context. */}
      <div className="flex flex-col gap-1.5">
        <FieldCaption>{t("approval.amount")}</FieldCaption>
        <p className="text-4xl font-semibold leading-none tabular-nums text-foreground">
          {/* The wire string, digit for digit - `formatUsdt` caps at 6 dp and parses
              through a float, and `payload_hash` covers the exact decimal. */}
          {formatExactUsdt(payout?.amount)}
          <span className="ml-2 text-base font-medium text-muted-foreground">USDT</span>
        </p>
      </div>

      <FullAddress label={t("approval.destination")} address={payout?.address ?? ""} />

      <div className="flex flex-col gap-2.5">
        <DetailRow label={t("approval.network")} value={networkLabel(payout?.network)} />
        {payout?.memo ? <DetailRow label={t("approval.memo")} value={payout.memo} mono /> : null}
        <DetailRow label={t("approval.payloadHash")} value={hashPrefix(payloadHash)} mono />
      </div>

      <p className="text-xs text-muted-foreground">{t("approval.payloadHashHint")}</p>

      <Separator />
    </>
  );
}
