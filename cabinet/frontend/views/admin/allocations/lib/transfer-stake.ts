// The stake-transfer form's model: what the operator has typed, whether it can be sent,
// and the exact wire body it becomes. Pure and React-free, like `./issuance.ts`, so the
// rules that matter — the recipient is always a person, the figure never exceeds what
// the company holds, a retry re-sends the same key — are tested here rather than
// discovered against the BFF's 400s.

// Relative and with the extension: the node test runner resolves no `@/` alias.
import type { TransferStakeBody } from "../../../../shared/contracts/admin.ts";
import { toBaseUnits } from "../../../../shared/lib/money.ts";

import { isDecimal, isPositive, keyForFingerprint, type SubmissionKey } from "./issuance.ts";

/** The investor the units land on, with the label the operator picked them by (their
 *  email, usually) so the confirmation and the result can say who, not just which id. */
export interface TransferRecipient {
  userId: string;
  label: string;
}

export interface TransferDraft {
  recipient: TransferRecipient | null;
  units: string;
  /** Empty means "let the hub default it to units × NAV" — sent as an ABSENT field, never
   *  as an empty string, which the hub would refuse as a malformed decimal. */
  costBasis: string;
}

export const EMPTY_TRANSFER_DRAFT: TransferDraft = { recipient: null, units: "", costBasis: "" };

/** The form once a transfer has landed: the recipient stays for the next hand-over in a
 *  series, the figures clear. The form retires its key at the same moment. */
export function afterTransferred(draft: TransferDraft): TransferDraft {
  return { ...draft, units: "", costBasis: "" };
}

/** Why the draft cannot be sent, in the order the form should point at. `exceeds` is the
 *  one rule the mint form has no counterpart for: the company can only hand over what it
 *  holds, and the hub's own 400 for it names nothing the operator can act on. */
export type TransferDraftProblem = "recipient" | "units" | "exceeds" | "costBasis";

export function transferDraftProblem(draft: TransferDraft, companyUnits: string | undefined): TransferDraftProblem | null {
  if (draft.recipient === null) return "recipient";
  if (!isPositive(draft.units)) return "units";
  // Exact comparison in base units, the way the hub compares — never on floats.
  if (toBaseUnits(draft.units) > toBaseUnits(companyUnits)) return "exceeds";
  if (draft.costBasis.trim() !== "" && !isDecimal(draft.costBasis)) return "costBasis";
  return null;
}

/** The wire body for a sendable draft, or `null` for one that is not — the form disables
 *  its button on the same rule, so a `null` here is a coding error and not a user one. */
export function transferStakeBody(service: string, draft: TransferDraft, companyUnits: string | undefined, idempotencyKey: string): TransferStakeBody | null {
  if (transferDraftProblem(draft, companyUnits) !== null || draft.recipient === null) return null;
  const costBasis = draft.costBasis.trim();
  return {
    service,
    user_id: draft.recipient.userId,
    units: draft.units.trim(),
    idempotency_key: idempotencyKey,
    ...(costBasis === "" ? {} : { cost_basis: costBasis }),
  };
}

/** Distinct submissions are judged on what would be sent — see `submissionKeyFor` in
 *  `./issuance.ts` for the contract. The recipient's label is not part of it: the same
 *  id under a changed display name is the same hand-over. */
export function transferFingerprint(service: string, draft: TransferDraft): string {
  return JSON.stringify([service, draft.recipient?.userId ?? null, draft.units.trim(), draft.costBasis.trim()]);
}

export function transferKeyFor(previous: SubmissionKey | null, service: string, draft: TransferDraft, mint?: () => string): SubmissionKey {
  return keyForFingerprint(previous, transferFingerprint(service, draft), mint);
}
