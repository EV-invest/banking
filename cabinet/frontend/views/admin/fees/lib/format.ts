// Words for a change of terms. Plain TypeScript — every helper that produces words takes
// the caller's `t`, the contract `views/admin/lib/format.ts` follows.

import type { Translate } from "@evinvest/i18n";

import { basisLabel, crystallizationLabel, type FeeTermsLike } from "@/shared/lib/fee-terms";
import { pct } from "@/shared/lib/rate";
import { wordFor } from "@/shared/lib/wire-words";

// Lives in `shared` since the admin resources poll on it (banking#326); the screen keeps
// reading it from here beside the other words for a change.
export { isPendingChange } from "@/shared/lib/fee-terms";

// The states this build has a name for. Anything else falls back to the bare wire word,
// which is legible — `views/consilium/lib/format.ts` makes the same call, for the same
// reason: a reader seeing `reconciling` is better served than `admin.feeChange.state.reconciling`.
const stateWords = (t: Translate): Readonly<Record<string, string>> => ({
  awaiting_consilium: t("admin.feeChange.state.awaiting_consilium", "Awaiting owners"),
  scheduled: t("admin.feeChange.state.scheduled", "Scheduled"),
  active: t("admin.feeChange.state.active", "In force"),
  superseded: t("admin.feeChange.state.superseded", "Superseded"),
  rejected: t("admin.feeChange.state.rejected", "Rejected"),
  cancelled: t("admin.feeChange.state.cancelled", "Cancelled"),
});

export function changeStateLabel(state: string | undefined, t: Translate): string {
  return wordFor(stateWords(t), state) ?? (state || "—");
}

// What set a charge off (fees.proto `FeeAssessment.trigger`): the period-end sweep, or an
// investor's redemption. Same guard-and-fall-back shape as the states above.
const triggerWords = (t: Translate): Readonly<Record<string, string>> => ({
  period: t("admin.fees.trigger.period", "Period"),
  redemption: t("admin.fees.trigger.redemption", "Redemption"),
});

export function triggerLabel(trigger: string | undefined, t: Translate): string {
  return wordFor(triggerWords(t), trigger) ?? (trigger || "—");
}

/** Token classes for a state pill. Neutral unless the state carries real news. */
export function changeStateTone(state: string | undefined): string {
  if (state === "active") return "text-positive";
  if (state === "scheduled" || state === "awaiting_consilium") return "text-accent-debug";
  if (state === "rejected") return "text-accent-error";
  return "text-ink-soft";
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
  return (terms.hurdle_bps > 0 ? t("admin.fees.summaryHurdle", "{management} p.a. · {performance} of the gain above {hurdle} · {basis} · {period}", words) : t("admin.fees.summary", "{management} p.a. · {performance} of the gain · {basis} · {period}", words));
}
