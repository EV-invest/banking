// The retirement form's model: what the operator has typed, whether it can be sent, and
// the exact wire body it becomes. Pure and React-free, like `./issuance.ts`, so the rules
// that matter — a body always names a person, a live product needs an explicit override,
// a retry re-sends the same key — are tested here rather than discovered against the
// BFF's 400s.

// Relative and with the extension: the node test runner resolves no `@/` alias.
import type { AllocationState, RetireUnitsBody } from "../../../../shared/contracts/admin.ts";

import { isDecimal, isPositive, keyForFingerprint, type IssueHolder, type SubmissionKey } from "./issuance.ts";

export interface RetireDraft {
  /** The same shape the mint form picks — the burn is its mirror, and always a person:
   *  the `fee` allocation's holding of a product is the fee accrual's, never burnt here. */
  holder: IssueHolder | null;
  units: string;
  /** Empty means "let the hub default it to units × NAV" — sent as an ABSENT field, never
   *  as an empty string, which the hub would refuse as a malformed decimal. */
  costBasis: string;
  /** The operator's override for a product that is not closed. Carried on the draft so a
   *  retry after a failure re-sends the same decision, and so the fingerprint sees it. */
  force: boolean;
}

export const EMPTY_RETIRE_DRAFT: RetireDraft = { holder: null, units: "", costBasis: "", force: false };

/** The form once a retirement has landed: the holder stays for the next burn in a
 *  series, the figures clear, and the override is spent — a second forced burn is a
 *  second decision, not a habit. */
export function afterRetired(draft: RetireDraft): RetireDraft {
  return { ...draft, units: "", costBasis: "", force: false };
}

/** Retiring is for a wound-down product. A `closed` one needs nothing more; a live one
 *  (draft or open) is refused by the hub unless the operator forces it. */
export function retireAllowed(state: AllocationState, force: boolean): boolean {
  return state === "closed" || force;
}

/** Why the draft cannot be sent, in the order the form should point at. No cap against
 *  the holding here: what a person has AVAILABLE (settled minus escrowed) is the hub's to
 *  know, and its refusal names the figure. */
export type RetireDraftProblem = "holder" | "units" | "costBasis";

export function retireDraftProblem(draft: RetireDraft): RetireDraftProblem | null {
  if (draft.holder === null) return "holder";
  if (!isPositive(draft.units)) return "units";
  if (draft.costBasis.trim() !== "" && !isDecimal(draft.costBasis)) return "costBasis";
  return null;
}

/** The wire body for a sendable draft, or `null` for one that is not — the form disables
 *  its button on the same rule, so a `null` here is a coding error and not a user one.
 *  `force` is sent only when the product needs it: a closed product never carries the
 *  override, even if the operator had ticked it before closing. */
export function retireUnitsBody(service: string, state: AllocationState, draft: RetireDraft, idempotencyKey: string): RetireUnitsBody | null {
  if (retireDraftProblem(draft) !== null || draft.holder === null || !retireAllowed(state, draft.force)) return null;
  const costBasis = draft.costBasis.trim();
  return {
    service,
    user_id: draft.holder.userId,
    units: draft.units.trim(),
    idempotency_key: idempotencyKey,
    ...(costBasis === "" ? {} : { cost_basis: costBasis }),
    ...(state === "closed" ? {} : { force: true as const }),
  };
}

/** Distinct submissions are judged on what would be sent — see `submissionKeyFor` in
 *  `./issuance.ts` for the contract. The override is part of it: a burn the operator had
 *  to force is not a retry of the one they did not. */
export function retireFingerprint(service: string, draft: RetireDraft): string {
  return JSON.stringify([service, draft.holder?.userId ?? null, draft.units.trim(), draft.costBasis.trim(), draft.force]);
}

export function retireKeyFor(previous: SubmissionKey | null, service: string, draft: RetireDraft, mint?: () => string): SubmissionKey {
  return keyForFingerprint(previous, retireFingerprint(service, draft), mint);
}
