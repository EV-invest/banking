"use client";

// A NAV-mark consilium's terms, as the owners' room shows them beside the tally: which
// product, the AUM the owners are asked to mark it at, and the one sentence that makes this
// a vote rather than a form — executing it records the mark whatever the NAV-move guard
// would have said. That guard has no per-request override any more (banking#232); this
// consilium IS the only way past it, so what the owners see here is the whole of it.

import type { Translate } from "@evinvest/i18n";
import { useT } from "@evinvest/i18n/react";

import { allocationsResource } from "@/entities/fund/model/fund-resource";
import type { ValuationOverride } from "@/shared/contracts/governance";
import { useResource } from "@/shared/lib/resource";

export function ValuationOverrideTerms({ terms }: { terms: ValuationOverride }) {
  const t = useT();
  // The investor catalog, which the rail has already asked for on every signed-in screen —
  // so this is a cache hit, not a second request. Through the hook rather than `peek()`:
  // the hook renders the server snapshot during hydration, a bare peek would not, and the
  // slug is a perfectly legible fallback while the title is not there yet.
  const catalog = useResource(allocationsResource);
  const hit = catalog.data?.allocations?.find((a) => a.service === terms.service);
  const product = hit?.title ? `${hit.title} (${terms.service})` : terms.service;
  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-col gap-1">
        <span className="text-xs font-medium text-muted-foreground">{t("consilium.valuation.product")}</span>
        <span className="text-sm font-medium text-foreground">{product}</span>
      </div>
      <p className="text-xs text-muted-foreground">{t("consilium.valuation.executes")}</p>
    </div>
  );
}

/** The one-line name of a settled NAV mark in the room's history: "NAV mark · trading". */
export function valuationWords(terms: ValuationOverride, t: Translate): string {
  return `${t("consilium.valuation.mark")} · ${terms.service}`;
}
