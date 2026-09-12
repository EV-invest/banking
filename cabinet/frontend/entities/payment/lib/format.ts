// The words for a payment order's wire vocabulary — state, tier, requirement, consent —
// shared by every surface that shows an order: the console list, the owners' room, the
// emailed approvals. Each takes the caller's `t`, as `views/admin/lib/format.ts` does,
// because this is a plain module with no translator of its own.
//
// Every lookup is guarded by a set and falls back to the bare wire word: the state
// machine lives in the money plane, and a state it grows tomorrow should render as
// `reconciling`, not as `payment.state.reconciling` (see `shared/contracts/payments.ts`).

import type { Translate } from "@evinvest/i18n";

import type { PaymentConsent } from "@/shared/contracts/payments";
import { settledConsent } from "@/shared/lib/decision";

const KNOWN_STATES: ReadonlySet<string> = new Set(["pending", "approved", "executed", "execution_failed", "rejected", "expired", "cancelled"]);

const KNOWN_TIERS: ReadonlySet<string> = new Set(["internal", "service", "external"]);

const KNOWN_REQUIREMENTS: ReadonlySet<string> = new Set(["owner_consilium", "subject_consent"]);

/** Still waiting on an answer — the only state the initiator can still cancel. */
export function isPaymentOpen(state: string): boolean {
  return state === "pending";
}

export function paymentStateLabel(state: string, t: Translate): string {
  return KNOWN_STATES.has(state) ? t(`payment.state.${state}`) : state;
}

/** Token classes for a state pill. Neutral unless the state carries real news. */
export function paymentStateTone(state: string): string {
  switch (state) {
    case "executed":
      return "text-main-accent-t2";
    case "pending":
    case "approved":
      return "text-main-accent-t1";
    case "rejected":
    case "execution_failed":
      return "text-destructive";
    default:
      return "text-muted-foreground";
  }
}

export function tierLabel(tier: string, t: Translate): string {
  return KNOWN_TIERS.has(tier) ? t(`payment.tier.${tier}`) : tier;
}

export function requirementLabel(requirement: string, t: Translate): string {
  return KNOWN_REQUIREMENTS.has(requirement) ? t(`payment.requirement.${requirement}`) : requirement;
}

/**
 * Where the one emailed seat stands, in words.
 *
 * `invalidated` is read before `decision`: a seat whose pins moved cannot be answered
 * whatever the decision field says, and "approved" on a seat the plane no longer trusts
 * would be the wrong news. An unanswered seat is `settledConsent`'s null, never the
 * truthy "pending" string.
 */
export function consentLabel(consent: PaymentConsent, t: Translate): string {
  if (consent.invalidated) return t("payment.consent.invalidated");
  const settled = settledConsent(consent.decision);
  if (settled === "approve") return t("payment.consent.approve");
  if (settled === "reject") return t("payment.consent.reject");
  return consent.notified ? t("payment.consent.pending") : t("payment.consent.unsent");
}

export function consentTone(consent: PaymentConsent): string {
  if (consent.invalidated) return "text-destructive";
  const settled = settledConsent(consent.decision);
  if (settled === "approve") return "text-main-accent-t2";
  if (settled === "reject") return "text-destructive";
  return "text-main-accent-t1";
}
