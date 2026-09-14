"use client";

// The form's state for one product's next terms: seeded from the policy in force (or the
// house default), edited a field at a time, reset after a change is scheduled.

import { useCallback, useState } from "react";

import { HOUSE_TERMS, type FeeTermsLike } from "@/shared/lib/fee-terms";
import { toPercentInput } from "@/shared/lib/rate";
import type { TermsDraft } from "@/views/admin/fees/lib/schedule";

/** The stored policy is basis points; the form is percent. Seeding through
 *  `toPercentInput` is what keeps that round trip lossless — a 2.55% rate opens on
 *  "2.55" and schedules back as the same 255 bps if the operator never touches it. */
export function draftFrom(terms: FeeTermsLike | null): TermsDraft {
  const seed = terms ?? HOUSE_TERMS;
  return {
    management: toPercentInput(seed.management_bps),
    performance: toPercentInput(seed.performance_bps),
    hurdle: toPercentInput(seed.hurdle_bps),
    basis: seed.basis,
    crystallization: seed.crystallization,
    effectiveFrom: "",
    reason: "",
  };
}

export function useTermsDraft(current: FeeTermsLike | null) {
  const [draft, setDraft] = useState<TermsDraft>(() => draftFrom(current));
  const set = useCallback(<K extends keyof TermsDraft>(field: K, value: TermsDraft[K]) => {
    setDraft((prev) => ({ ...prev, [field]: value }));
  }, []);
  const reset = useCallback(() => setDraft(draftFrom(current)), [current]);
  return { draft, set, reset };
}
