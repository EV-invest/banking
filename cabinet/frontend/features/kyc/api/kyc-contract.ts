// The wire contract of the identity plane's two KYC routes, as the browser sees it.
//
// `/kyc/start` and `/kyc/status` are not in `contracts/openapi.json` — they are web-surface
// routes on concierge, not gRPC, so no generated type exists and none is coming. What the
// slice used to do instead was hand-probe each answer at the point of use: `readString(data,
// "redirect_url")` in one place, `readString(error.body, "contact")` in another. A rename on
// the plane's side turned into "please try again" in front of a healthy provider.
//
// So the shape is written down HERE, once, as narrowing functions. Their counterpart is the
// integration test in concierge (`runner/tests/kyc.rs`), which compares whole JSON documents
// against the same shapes — a rename fails there, and is refused here.
//
// No schema library: four shapes do not earn a dependency in `package.json`, and the probing
// they need is three lines each (see `../lib/read-field`).

import { hasField, readBoolean, readNumber, readObject, readString } from "../lib/read-field.ts";

/**
 * The closed dictionary of `{ "error": … }` codes both routes publish. Anything outside it
 * is drift, and is read as "no code at all" rather than guessed at.
 *
 * `kyc_unavailable` is the only one carrying a second field (`contact`), and the only one
 * whose meaning is a designed state rather than a fault.
 */
export const KYC_ERROR_CODES = ["unauthenticated", "csrf", "throttled", "internal", "kyc_unavailable"] as const;

export type KycErrorCode = (typeof KYC_ERROR_CODES)[number];

export interface KycErrorBody {
  error: KycErrorCode;
  /** Present only on `kyc_unavailable`; `null` on every other code and when omitted. */
  contact: string | null;
}

export interface KycStartResponse {
  redirectUrl: string;
  caseId: string | null;
}

/**
 * A verification attempt that is still running. A decided case is history and never appears
 * here — the plane drops it from the answer the moment it resolves.
 */
export interface KycCase {
  /**
   * The plane's own status word. Kept as an open string on purpose: today it is one of
   * {@link RUNNING_CASE_STATUSES}, but the column is a persistent dictionary that may grow a
   * value before this cabinet ships again. Rejecting an unrecognised word would make the
   * whole status unreadable, and an unreadable status falls back to offering Start — i.e. a
   * new running state would buy a second paid vendor session. A word we do not recognise
   * still means a case is running, which is the only thing the gate needs.
   */
  status: string;
  /** Always the entry tier (1) today; carried rather than assumed. */
  requestedTier: number;
  /** Unix SECONDS, not milliseconds — pinned by the concierge test. */
  createdAt: number;
  /**
   * Whether the vendor session behind this case can still be re-entered. `false` means the
   * case is alive and holds the start gate, but there is no link to send the user back to:
   * the cabinet must offer a fresh start, never a dead link.
   */
  resumable: boolean;
}

export interface KycStatus {
  level: number;
  case: KycCase | null;
}

/** The statuses a running case can carry today. Informational — see {@link KycCase.status}. */
export const RUNNING_CASE_STATUSES = ["pending", "in_progress", "in_review", "resubmitted"] as const;

/** `null` when the body is not a usable start answer — a missing or empty `redirect_url`. */
export function parseStartResponse(body: unknown): KycStartResponse | null {
  const redirectUrl = readString(body, "redirect_url");
  if (redirectUrl === null) return null;
  return { redirectUrl, caseId: readString(body, "case_id") };
}

/**
 * `null` when the body is not a status document. Callers treat that exactly as they treat a
 * 404 from a plane that has not shipped the route yet: fall back to the tier alone.
 */
export function parseStatus(body: unknown): KycStatus | null {
  const level = readNumber(body, "level");
  if (level === null) return null;
  // `case: null` is the documented "nothing running", not a parse failure; a `case` key that
  // is present and malformed IS one, and takes the whole document down with it.
  const raw = readObject(body, "case");
  if (raw === null) return hasField(body, "case") ? { level, case: null } : null;
  const kycCase = parseCase(raw);
  return kycCase === null ? null : { level, case: kycCase };
}

function parseCase(raw: unknown): KycCase | null {
  const status = readString(raw, "status");
  const requestedTier = readNumber(raw, "requested_tier");
  const createdAt = readNumber(raw, "created_at");
  const resumable = readBoolean(raw, "resumable");
  if (status === null || requestedTier === null || createdAt === null || resumable === null) return null;
  return { status, requestedTier, createdAt, resumable };
}

/**
 * `null` when the body carries no code from {@link KYC_ERROR_CODES}.
 *
 * Both routes now answer every refusal with a body, which retires the old heuristic ("a 403
 * without a body is a stale token, a 403 with one is a refusal on the merits") — there is no
 * bodyless case left to key on.
 */
export function parseErrorBody(body: unknown): KycErrorBody | null {
  const error = readString(body, "error");
  if (error === null || !isKycErrorCode(error)) return null;
  return { error, contact: error === "kyc_unavailable" ? readString(body, "contact") : null };
}

function isKycErrorCode(value: string): value is KycErrorCode {
  return (KYC_ERROR_CODES as readonly string[]).includes(value);
}
