"use client";

// A NAV mark's terms, as the owners' approval page shows them: the AUM and the product get
// the top of the card, because those two are exactly what `payload_hash` covers and what
// the reader is agreeing to (policy 12–13). A sibling of `payment-terms.tsx` — a
// consilium carries exactly one kind's terms.
//
// The product is its slug only. This page is reached from an email with no session, so
// there is no catalog to resolve a title from — and the slug is what the digest binds.

import { useLocale, useT } from "@evinvest/i18n/react";
import { Separator } from "@evinvest/uikit";

import type { ValuationOverride } from "@/shared/contracts/governance";
import { hashPrefix } from "@/shared/lib/hash";
import { formatExactUsdt } from "@/shared/lib/money";
import { DetailRow, FieldCaption } from "@/views/approval/ui/approval-chrome";

/**
 * Both halves of the digest must actually be on screen before the code field is. The BFF
 * fills a missing sibling with empty strings, which are not nullish — so a request whose
 * AUM or product did not arrive is not offered for decision at all (policy 12–13).
 */
export function renderableValuation(terms: ValuationOverride | null | undefined): terms is ValuationOverride {
  return Boolean(terms?.aum?.trim()) && Boolean(terms?.service?.trim());
}

export function ValuationTermsBlock({ terms, payloadHash }: { terms: ValuationOverride; payloadHash: string }) {
  const t = useT();
  const locale = useLocale();
  return (
    <>
      <div className="flex flex-col gap-1.5">
        <FieldCaption>{t("approval.valuation.aum", "Proposed AUM")}</FieldCaption>
        <p className="text-4xl font-semibold leading-none tabular-nums text-ink">
          {/* The wire string, digit for digit — `payload_hash` covers the exact decimal. */}
          {formatExactUsdt(terms.aum, locale)}
          <span className="ml-2 text-base font-medium text-ink-soft">USDT</span>
        </p>
      </div>

      <div className="flex flex-col gap-2.5">
        <DetailRow label={t("approval.valuation.product", "Product")} value={terms.service} mono />
        <DetailRow label={t("approval.payloadHash", "Request fingerprint")} value={hashPrefix(payloadHash)} mono />
      </div>

      <p className="text-xs text-ink-soft">{t("approval.valuation.executes", "If it passes, the fund is marked at this AUM whatever the NAV-move guard would have said, and its queued redemptions settle at the NAV that follows.")}</p>
      <p className="text-xs text-ink-soft">{t("approval.valuation.payloadHashHint", "The fingerprint is taken over the product and the AUM above. It is checked again before the mark is recorded, so a request cannot be edited after you have agreed to it.")}</p>

      <Separator />
    </>
  );
}
