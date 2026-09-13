"use client";

// The registry row's State cell — its own file for the same reason the Access and
// Actions cells are: one cell per file keeps `AllocationRow` itself under the
// component-size ceiling as columns grow.

import { useT } from "@evinvest/i18n/react";

import type { AllocationState } from "@/shared/contracts/admin";
import { cn } from "@/shared/lib/cn";
import { stateLabel } from "@/views/admin/lib/format";

// `closed` is amber rather than destructive: it stops new subscriptions but investors
// can still redeem out of it, so it is a wind-down, not a failure.
const STATE_TONE: Record<AllocationState, string> = {
  draft: "border-border text-muted-foreground",
  open: "border-main-accent-t2/40 bg-main-accent-t2/10 text-main-accent-t2",
  closed: "border-main-accent-t3/40 bg-main-accent-t3/10 text-main-accent-t3",
};

// Catalogue keys, not finished prose: this map is module scope, so it holds what to say
// rather than the words, and the cell resolves it against the reader's locale.
const STATE_HINT: Record<AllocationState, string> = {
  draft: "admin.alloc.hint.draft",
  open: "admin.alloc.hint.open",
  closed: "admin.alloc.hint.closed",
};

export function AllocationStateCell({ state }: { state: AllocationState }) {
  const t = useT();
  return (
    // i18n-max: 12 — a chip in a table cell. Display case, not lowercased-and-
    // `capitalize`d: that rule title-cases every word, invisible on a one-word English
    // enum and wrong once translated ("en cours" → "En Cours").
    <span className={cn("inline-flex items-center whitespace-nowrap rounded-full border px-2 py-0.5 text-xs font-medium", STATE_TONE[state])} title={t(STATE_HINT[state])}>
      {stateLabel(state, t)}
    </span>
  );
}
