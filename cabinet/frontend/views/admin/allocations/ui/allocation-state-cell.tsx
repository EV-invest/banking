"use client";

// The registry row's State cell — its own file for the same reason the Access and
// Actions cells are: one cell per file keeps `AllocationRow` itself under the
// component-size ceiling as columns grow.

import type { Translate } from "@evinvest/i18n";
import { useT } from "@evinvest/i18n/react";

import type { AllocationState } from "@/shared/contracts/admin";
import { cn } from "@/shared/lib/cn";
import { stateLabel } from "@/views/admin/lib/format";

// `closed` is amber rather than destructive: it stops new subscriptions but investors
// can still redeem out of it, so it is a wind-down, not a failure.
const STATE_TONE: Record<AllocationState, string> = {
  draft: "border-border text-ink-soft",
  open: "border-positive/40 bg-positive/10 text-positive",
  closed: "border-accent-warn/40 bg-accent-warn/10 text-accent-warn",
};

// Catalogue keys, not finished prose: this map is module scope, so it holds what to say
// rather than the words, and the cell resolves it against the reader's locale.
const stateHints = (t: Translate): Record<AllocationState, string> => ({
  draft: t("admin.alloc.hint.draft", "Registered — accepts no money until opened"),
  open: t("admin.alloc.hint.open", "Accepting subscriptions and redemptions"),
  closed: t("admin.alloc.hint.closed", "Closed to new money; redemptions still settle"),
});

export function AllocationStateCell({ state }: { state: AllocationState }) {
  const t = useT();
  return (
    // i18n-max: 12 — a chip in a table cell. Display case, not lowercased-and-
    // `capitalize`d: that rule title-cases every word, invisible on a one-word English
    // enum and wrong once translated ("en cours" → "En Cours").
    <span className={cn("inline-flex items-center whitespace-nowrap rounded-full border px-2 py-0.5 text-xs font-medium", STATE_TONE[state])} title={stateHints(t)[state]}>
      {stateLabel(state, t)}
    </span>
  );
}
