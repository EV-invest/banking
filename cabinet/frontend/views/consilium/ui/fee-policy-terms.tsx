"use client";

// A fee-policy consilium's terms, as the owners' room shows them beside the tally: the
// product, the five fields now and proposed, when the change would bind, how many holders
// the notice reaches, and the requester's reason. The third sibling of `payment-terms.tsx`
// — a consilium carries exactly one of the three.

import { useLocale, useT } from "@evinvest/i18n/react";

import { FeeTermsDiff } from "@/entities/fund/ui/fee-terms-diff";
import type { ConsiliumFeePolicyTerms } from "@/shared/contracts/governance";
import { formatMoment, hasStamp } from "@/shared/lib/datetime";
import { pct } from "@/shared/lib/rate";

export function FeePolicyTerms({ terms }: { terms: ConsiliumFeePolicyTerms }) {
  const t = useT();
  const locale = useLocale();
  return (
    <div className="flex flex-col gap-3">
      <FeeTermsDiff from={terms.from} to={terms.to} />
      <div className="flex flex-col gap-1.5 text-xs text-ink-soft">
        <span className="tabular-nums">
          {t("consilium.feePolicy.effectiveFrom", {
            // "0" is "as soon as allowed": the 24h notice is counted from the owners'
            // approval, so the moment the terms bind is not known until they carry it.
            at: hasStamp(terms.effective_from) ? formatMoment(terms.effective_from, locale) : t("consilium.feePolicy.asSoonAsAllowed"),
          })}
        </span>
        <span className="tabular-nums">{t("consilium.feePolicy.holders", { n: terms.holder_count })}</span>
      </div>
      <div className="flex flex-col gap-1">
        <span className="text-xs font-medium text-ink-soft">{t("consilium.feePolicy.reason")}</span>
        {/* The requester's words, whole, set apart from the room's own. */}
        <blockquote className="whitespace-pre-line border-l-2 border-main-accent-t3/60 pl-3 text-sm leading-relaxed text-ink">{terms.reason?.trim() || "—"}</blockquote>
      </div>
    </div>
  );
}

/** The one-line name of a settled change in the room's history: "Alpha · 2% / 20% → 3% / 25%". */
export function feePolicyWords(terms: ConsiliumFeePolicyTerms): string {
  const rates = (x: { management_bps: number; performance_bps: number }) => `${pct(x.management_bps)} / ${pct(x.performance_bps)}`;
  return `${terms.allocation_name || terms.service || "—"} · ${terms.from ? rates(terms.from) : "—"} → ${rates(terms.to)}`;
}
