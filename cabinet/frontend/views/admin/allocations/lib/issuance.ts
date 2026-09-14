// The in-kind issue form's model: what the operator has typed, whether it can be sent,
// and the exact wire body it becomes. Pure and React-free so the two rules that matter —
// a body never names nobody, and a retry re-sends the same key — are tested here rather
// than discovered against the BFF's 400s.

import type { IssueUnitsBody } from "@/entities/admin/api/admin-client";

/** Who the units land on. A user carries the label the operator picked them by (their
 *  email, usually) so the result line can say who, not just which id. */
export type IssueHolder = { kind: "user"; userId: string; label: string } | { kind: "company" };

export interface IssueDraft {
  holder: IssueHolder | null;
  units: string;
  /** Empty means "let the hub default it to units × NAV" — sent as an ABSENT field, never
   *  as an empty string, which the hub would refuse as a malformed decimal. */
  costBasis: string;
}

export const EMPTY_ISSUE_DRAFT: IssueDraft = { holder: null, units: "", costBasis: "" };

/** Why the draft cannot be sent, in the order the form should point at: the holder is the
 *  one thing the form cannot guess, then the figure that must be positive, then the one
 *  that need only be well-formed. `null` means send it. */
export type IssueDraftProblem = "holder" | "units" | "costBasis";

// A decimal amount as the wire carries it. No sign, no exponent, no grouping — the hub
// parses this with the same strictness, so admitting less here only moves the refusal.
const DECIMAL = /^\d+(\.\d+)?$/;

const isDecimal = (raw: string): boolean => DECIMAL.test(raw.trim());

const isPositive = (raw: string): boolean => isDecimal(raw) && /[1-9]/.test(raw);

export function issueDraftProblem(draft: IssueDraft): IssueDraftProblem | null {
  if (draft.holder === null) return "holder";
  if (!isPositive(draft.units)) return "units";
  // Zero is a legitimate basis — the company's stake in an asset it already owned cost
  // it nothing — so this only asks that the field parse, not that it be positive.
  if (draft.costBasis.trim() !== "" && !isDecimal(draft.costBasis)) return "costBasis";
  return null;
}

/** The wire body for a sendable draft, or `null` for one that is not — the form disables
 *  its button on the same rule, so a `null` here is a coding error and not a user one. */
export function issueUnitsBody(service: string, draft: IssueDraft, idempotencyKey: string): IssueUnitsBody | null {
  if (issueDraftProblem(draft) !== null || draft.holder === null) return null;
  const costBasis = draft.costBasis.trim();
  const base = {
    service,
    units: draft.units.trim(),
    idempotency_key: idempotencyKey,
    ...(costBasis === "" ? {} : { cost_basis: costBasis }),
  };
  return draft.holder.kind === "company" ? { ...base, company: true } : { ...base, user_id: draft.holder.userId };
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
  return JSON.stringify([service, draft.holder, draft.units.trim(), draft.costBasis.trim()]);
}

export function submissionKeyFor(previous: SubmissionKey | null, service: string, draft: IssueDraft, mint: () => string = () => crypto.randomUUID()): SubmissionKey {
  const fingerprint = submissionFingerprint(service, draft);
  if (previous && previous.fingerprint === fingerprint) return previous;
  return { key: mint(), fingerprint };
}
