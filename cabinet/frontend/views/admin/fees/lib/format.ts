// Words for a change of terms. Plain TypeScript — every helper that produces words takes
// the caller's `t`, the contract `views/admin/lib/format.ts` follows.

import type { Translate } from "@evinvest/i18n";

import { basisLabel, crystallizationLabel, type FeeTermsLike } from "@/shared/lib/fee-terms";
import { pct } from "@/shared/lib/rate";

// The states this build has a name for. Anything else falls back to the bare wire word,
// which is legible — `views/consilium/lib/format.ts` makes the same call, for the same
// reason: a reader seeing `reconciling` is better served than `admin.feeChange.state.reconciling`.
const KNOWN_STATES: ReadonlySet<string> = new Set(["awaiting_consilium", "scheduled", "active", "superseded", "rejected", "cancelled"]);

export function changeStateLabel(state: string | undefined, t: Translate): string {
  return state && KNOWN_STATES.has(state) ? t(`admin.feeChange.state.${state}`) : state || "—";
}

// What set a charge off (fees.proto `FeeAssessment.trigger`): the period-end sweep, or an
// investor's redemption. Same guard-and-fall-back shape as the states above.
const KNOWN_TRIGGERS: ReadonlySet<string> = new Set(["period", "redemption"]);

export function triggerLabel(trigger: string | undefined, t: Translate): string {
  return trigger && KNOWN_TRIGGERS.has(trigger) ? t(`admin.fees.trigger.${trigger}`) : trigger || "—";
}

/** Token classes for a state pill. Neutral unless the state carries real news. */
export function changeStateTone(state: string | undefined): string {
  if (state === "active") return "text-main-accent-t2";
  if (state === "scheduled" || state === "awaiting_consilium") return "text-main-accent-t1";
  if (state === "rejected") return "text-destructive";
  return "text-muted-foreground";
}

/** Pending means the change is still on its way — the two states a cancel can reach. */
export function isPendingChange(state: string | undefined): boolean {
  return state === "scheduled" || state === "awaiting_consilium";
}

/** The five terms in one line: "2% p.a. · 20% of the gain above 5% · Invested capital · Annually". */
export function termsSummary(terms: FeeTermsLike, t: Translate): string {
  const words = {
    management: pct(terms.management_bps),
    performance: pct(terms.performance_bps),
    hurdle: pct(terms.hurdle_bps),
    basis: basisLabel(terms.basis, t),
    period: crystallizationLabel(terms.crystallization, t),
  };
  return t(terms.hurdle_bps > 0 ? "admin.fees.summaryHurdle" : "admin.fees.summary", words);
}
