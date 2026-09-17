// The in-kind issue form's model: what the operator has typed, whether it can be sent,
// and the exact wire body it becomes. Pure and React-free so the two rules that matter —
// a body never names nobody, and a retry re-sends the same key — are tested here rather
// than discovered against the BFF's 400s.

import type { IssueUnitsBody } from "@/entities/admin/api/admin-client";

/** Who the units land on — always a person (#245): the company is not a holder, and the
 *  reserved `fee` / `fund` allocations are seated by the owners' consilium, never by an
 *  operator's mint. Carries the label the operator picked them by (their email, usually)
 *  so the result line can say who, not just which id. */
export interface IssueHolder {
  userId: string;
  label: string;
}

export interface IssueDraft {
  holder: IssueHolder | null;
  units: string;
  /** Empty means "let the hub default it to units × NAV" — sent as an ABSENT field, never
   *  as an empty string, which the hub would refuse as a malformed decimal. */
  costBasis: string;
}

export const EMPTY_ISSUE_DRAFT: IssueDraft = { holder: null, units: "", costBasis: "" };

/** The form once a mint has landed. The holder stays — a series of issues to one person
 *  must not send the operator back to the picker each time — and only the figures clear.
 *  The form retires its submission key at the same moment (see `submissionKeyFor`): the
 *  next body is a new decision even when it is byte-identical, which it is exactly when
 *  the same holder is issued the same amount twice on purpose. */
export function afterIssued(draft: IssueDraft): IssueDraft {
  return { ...draft, units: "", costBasis: "" };
}

/** Why the draft cannot be sent, in the order the form should point at: the holder is the
 *  one thing the form cannot guess, then the figure that must be positive, then the one
 *  that need only be well-formed. `null` means send it. */
export type IssueDraftProblem = "holder" | "units" | "costBasis";

// A decimal amount as the wire carries it. No sign, no exponent, no grouping — the hub
// parses this with the same strictness, so admitting less here only moves the refusal.
const DECIMAL = /^\d+(\.\d+)?$/;

export const isDecimal = (raw: string): boolean => DECIMAL.test(raw.trim());

export const isPositive = (raw: string): boolean => isDecimal(raw) && /[1-9]/.test(raw);

export function issueDraftProblem(draft: IssueDraft): IssueDraftProblem | null {
  if (draft.holder === null) return "holder";
  if (!isPositive(draft.units)) return "units";
  // Zero is a legitimate basis — a stake in an asset the holder already owned cost them
  // nothing — so this only asks that the field parse, not that it be positive.
  if (draft.costBasis.trim() !== "" && !isDecimal(draft.costBasis)) return "costBasis";
  return null;
}

/** The wire body for a sendable draft, or `null` for one that is not — the form disables
 *  its button on the same rule, so a `null` here is a coding error and not a user one. */
export function issueUnitsBody(service: string, draft: IssueDraft, idempotencyKey: string): IssueUnitsBody | null {
  if (issueDraftProblem(draft) !== null || draft.holder === null) return null;
  const costBasis = draft.costBasis.trim();
  return {
    service,
    user_id: draft.holder.userId,
    units: draft.units.trim(),
    idempotency_key: idempotencyKey,
    ...(costBasis === "" ? {} : { cost_basis: costBasis }),
  };
}

/**
 * The retry contract, kept by the form: one key per distinct submission, the SAME key on
 * a retry of it. "Distinct" is judged on what would be sent — a draft edited after a
 * failure is a new decision and gets a new key, while a resend of the identical body after
 * a timeout must reuse the old one, or the double click the key exists to absorb lands
 * two mints.
 *
 * `crypto.randomUUID()` is 36 characters, inside the hub's 1..64 bound.
 */
export interface SubmissionKey {
  key: string;
  fingerprint: string;
}

export function submissionFingerprint(service: string, draft: IssueDraft): string {
  // The label is display only — the same person under a changed name is the same mint.
  return JSON.stringify([service, draft.holder?.userId ?? null, draft.units.trim(), draft.costBasis.trim()]);
}

export function submissionKeyFor(previous: SubmissionKey | null, service: string, draft: IssueDraft, mint: () => string = () => crypto.randomUUID()): SubmissionKey {
  return keyForFingerprint(previous, submissionFingerprint(service, draft), mint);
}

/** The retry contract on its own, for any form whose body can be fingerprinted — the
 *  retirement (`./retire.ts`) shares the mint's key space and its rule. */
export function keyForFingerprint(previous: SubmissionKey | null, fingerprint: string, mint: () => string = () => crypto.randomUUID()): SubmissionKey {
  if (previous && previous.fingerprint === fingerprint) return previous;
  return { key: mint(), fingerprint };
}
