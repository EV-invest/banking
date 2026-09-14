"use client";

// A change of fee terms, as the owners' approval page shows it: the product gets the top
// of the card, the five fields now and proposed beneath, and everything past the separator
// is context. The third sibling of `payout-terms.tsx` and `payment-terms.tsx`.
//
// Policy 12–13 shape it: everything the digest covers — the product, both sets of terms,
// the reason and the requested moment — is on screen before the code field is.

import { useLocale, useT } from "@evinvest/i18n/react";
import { Separator } from "@evinvest/uikit";

import { FeeTermsDiff } from "@/entities/fund/ui/fee-terms-diff";
import type { ConsiliumFeePolicyTerms } from "@/shared/contracts/governance";
import { formatMoment, hasStamp } from "@/shared/lib/datetime";
import { hashPrefix } from "@/shared/lib/hash";
import { DetailRow, FieldCaption } from "@/views/approval/ui/approval-chrome";

/** The terms a reader is agreeing to must actually be on screen: a change whose product
 *  or proposed terms did not arrive is not offered for decision (policy 12–13). */
export function renderableFeePolicy(terms: ConsiliumFeePolicyTerms | null | undefined): terms is ConsiliumFeePolicyTerms {
  return Boolean(terms?.change_id?.trim()) && Boolean(terms?.service?.trim()) && terms?.to !== undefined && terms?.to !== null;
}

export function FeePolicyTermsBlock({ terms, payloadHash }: { terms: ConsiliumFeePolicyTerms; payloadHash: string }) {
  const t = useT();
  const locale = useLocale();
  return (
    <>
      <div className="flex flex-col gap-1.5">
        <FieldCaption>{t("consilium.feePolicy.fund")}</FieldCaption>
        <p className="text-2xl font-semibold leading-tight text-foreground">{terms.allocation_name || terms.service}</p>
      </div>

      <FeeTermsDiff from={terms.from} to={terms.to} />

      <div className="flex flex-col gap-1.5">
        <FieldCaption>{t("consilium.feePolicy.reason")}</FieldCaption>
        <blockquote className="whitespace-pre-line border-l-2 border-main-accent-t3/60 pl-3 text-sm leading-relaxed text-foreground">{terms.reason?.trim() || "—"}</blockquote>
      </div>

      <div className="flex flex-col gap-2.5">
        <DetailRow
          label={t("consilium.feePolicy.effectiveFromLabel")}
          value={hasStamp(terms.effective_from) ? formatMoment(terms.effective_from, locale) : t("consilium.feePolicy.asSoonAsAllowed")}
        />
        <DetailRow label={t("consilium.feePolicy.holdersLabel")} value={terms.holder_count} />
        <DetailRow label={t("approval.payloadHash")} value={hashPrefix(payloadHash)} mono />
      </div>

      <p className="text-xs text-muted-foreground">{t("approval.feePolicy.payloadHashHint")}</p>

      <Separator />
    </>
  );
}
