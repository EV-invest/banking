"use client";

// One line under the Fees card: the change of terms on its way, if any.
//
// Two facts, and they are worded apart. A SCHEDULED change has a moment — the holders
// have been mailed it, and this line repeats it. One AWAITING the owners has no moment
// yet: the 24h notice is counted from their approval, so the line says "proposed" and
// names nobody's date. Anything else (`active`, `cancelled`, a word this build does not
// know) is not on its way, and the card says nothing.

import { useLocale, useT } from "@evinvest/i18n/react";

import type { FeePolicy } from "@/shared/contracts";
import { formatMoment } from "@/shared/lib/datetime";
import { basisLabel, crystallizationLabel } from "@/shared/lib/fee-terms";
import { pct } from "@/shared/lib/rate";
import { Note } from "@/views/invest/ui/atoms";

export function FeePendingNote({ pending }: { pending: FeePolicy["pending"] }) {
  const t = useT();
  const locale = useLocale();
  if (!pending || (pending.state !== "scheduled" && pending.state !== "awaiting_consilium")) return null;
  // proto3 JSON drops a zero, so an absent rate here IS a zero rate, not a missing one.
  const words = {
    management: pct(pending.management_bps ?? 0),
    performance: pct(pending.performance_bps ?? 0),
    hurdle: pct(pending.hurdle_bps ?? 0),
    basis: basisLabel(pending.basis, t),
    period: crystallizationLabel(pending.crystallization, t),
  };
  const terms = t((pending.hurdle_bps ?? 0) > 0 ? "admin.fees.summaryHurdle" : "admin.fees.summary", words);
  return (
    <Note tone="accent">
      {pending.state === "scheduled"
        ? // The generated type admits a number for an int64; the wire carries a string.
          t("invest.feeChangeScheduled", { at: formatMoment(String(pending.effective_from ?? "0"), locale), terms })
        : t("invest.feeChangeProposed", { terms })}
    </Note>
  );
}
