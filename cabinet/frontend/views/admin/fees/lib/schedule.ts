// The form's draft of a change of terms, and what it says about itself before the click.
//
// Three facts are worked out here so the operator is not surprised by the plane's answer:
// whether every rate is a rate at all and under its ceiling, whether the owners will be
// asked (and a reason is therefore required), and what moment the request carries. The
// plane decides all three for real; this is the preview, pinned to its rules by
// `schedule.test.ts`.

// Kept free of path-alias VALUE imports: the test runs under Node's own resolver, which
// knows nothing of `@/`. The two modules this leans on are import-free themselves.

import type { ScheduleFeePolicyRequest } from "@/shared/contracts/admin";

import {
  MAX_EFFECTIVE_FROM_HORIZON_SECS,
  MAX_HURDLE_BPS,
  MAX_MANAGEMENT_BPS,
  MAX_PERFORMANCE_BPS,
  MAX_REASON_BYTES,
  MIN_NOTICE_SECS,
  requirementFor,
  type ChangeRequirement,
  type FeeTermsLike,
} from "../../../../shared/lib/fee-terms.ts";
import { pct, toBps } from "../../../../shared/lib/rate.ts";

/** The five terms as the form holds them: rates in percent, exactly as typed. */
export interface TermsDraft {
  management: string;
  performance: string;
  hurdle: string;
  basis: string;
  crystallization: string;
  /** A `datetime-local` value, or empty for "as soon as the notice allows". */
  effectiveFrom: string;
  reason: string;
}

export type RateField = "management" | "performance" | "hurdle";

export const RATE_FIELDS: readonly RateField[] = ["management", "performance", "hurdle"];

const CEILING_BPS: Record<RateField, number> = {
  management: MAX_MANAGEMENT_BPS,
  performance: MAX_PERFORMANCE_BPS,
  hurdle: MAX_HURDLE_BPS,
};

/** The label key of a rate field — `t(field...)` is the view's, so the sentence can put
 *  the field name at whichever end the language wants. */
export const FIELD_LABEL_KEY: Record<RateField, string> = {
  management: "admin.fees.field.management",
  performance: "admin.fees.field.performance",
  hurdle: "admin.fees.field.hurdle",
};

/** Why the draft cannot be sent, in message-key form; the view interpolates. */
export type DraftProblem =
  | { key: "admin.fees.err.notPercent"; field: RateField }
  | { key: "admin.fees.err.overCeiling"; field: RateField; ceiling: string }
  | { key: "admin.fees.err.reasonRequired" }
  | { key: "admin.fees.err.reasonTooLong"; max: number; used: number }
  | { key: "admin.fees.err.tooFarAhead"; days: number };

/** The horizon in the unit the sentence about it uses. */
export const MAX_HORIZON_DAYS = MAX_EFFECTIVE_FROM_HORIZON_SECS / 86_400;

/**
 * The reason as the wire will carry it: one line, trimmed. The plane refuses any control
 * character outright (`validate_reason` — a line break has every meaning in a mail
 * header), and the field is single-line, so the only way one arrives is a paste; it is
 * folded into a space rather than bounced back as a server error about "control characters".
 */
export function normalizeReason(reason: string): string {
  return reason.replace(/\p{Cc}+/gu, " ").trim();
}

/** How long the plane will measure the reason to be — bytes, not characters. */
export function reasonBytes(reason: string): number {
  return new TextEncoder().encode(reason).length;
}

/** Every rate parsed, or `null` while any of them is not a percent. */
export function draftBps(draft: TermsDraft): Record<RateField, number | null> {
  return { management: toBps(draft.management), performance: toBps(draft.performance), hurdle: toBps(draft.hurdle) };
}

/** The terms the draft describes, or `null` while a rate does not parse or breaks its
 *  ceiling — a request the plane would refuse is not a request. */
export function draftTerms(draft: TermsDraft): FeeTermsLike | null {
  const bps = draftBps(draft);
  for (const field of RATE_FIELDS) {
    const value = bps[field];
    if (value === null || value > CEILING_BPS[field]) return null;
  }
  return {
    management_bps: bps.management ?? 0,
    performance_bps: bps.performance ?? 0,
    hurdle_bps: bps.hurdle ?? 0,
    basis: draft.basis,
    crystallization: draft.crystallization,
  };
}

/**
 * Who will have to agree, as best the browser can tell. `null` while the draft is not yet
 * terms at all — the question has no answer until every rate parses.
 */
export function draftRequirement(current: FeeTermsLike | null, draft: TermsDraft): ChangeRequirement | null {
  const next = draftTerms(draft);
  return next ? requirementFor(current, next) : null;
}

/** The first thing wrong with the draft, in field order, or `null` when it can be sent.
 *  `nowSeconds` is the clock the horizon is measured from — the caller's, so a test can
 *  hold it still and a card can read it once rather than on every keystroke. */
export function draftProblem(current: FeeTermsLike | null, draft: TermsDraft, nowSeconds: number): DraftProblem | null {
  const bps = draftBps(draft);
  for (const field of RATE_FIELDS) {
    const value = bps[field];
    // "Not a percentage" and "more than the ceiling" are different mistakes and want
    // different words — which is why the ceiling is not enforced inside `toBps`.
    if (value === null) return { key: "admin.fees.err.notPercent", field };
    if (value > CEILING_BPS[field]) return { key: "admin.fees.err.overCeiling", field, ceiling: pct(CEILING_BPS[field]) };
  }
  // Only the far end is an error. An early moment is not one: the plane lifts it to the
  // notice floor rather than refusing it (`earliest_effective_from`), and the field says
  // so as a preview (`effectiveFromLifted`) instead of turning red.
  const moment = effectiveFromSeconds(draft.effectiveFrom);
  if (moment !== null && moment > nowSeconds + MAX_EFFECTIVE_FROM_HORIZON_SECS) return { key: "admin.fees.err.tooFarAhead", days: MAX_HORIZON_DAYS };
  const reason = normalizeReason(draft.reason);
  const used = reasonBytes(reason);
  if (used > MAX_REASON_BYTES) return { key: "admin.fees.err.reasonTooLong", max: MAX_REASON_BYTES, used };
  if (draftRequirement(current, draft) === "owner_consilium" && reason.length === 0) {
    return { key: "admin.fees.err.reasonRequired" };
  }
  return null;
}

/**
 * A `datetime-local` value → unix seconds, in the operator's own zone — that is the zone
 * the field was typed in. Empty is `0`, the wire's "as soon as the notice allows"; a value
 * the browser cannot read (never from the native control, always from a script) is `null`
 * rather than a silent zero, because "now" is not what someone who typed a date meant.
 */
export function effectiveFromSeconds(value: string): number | null {
  if (value.trim() === "") return 0;
  const ms = new Date(value).getTime();
  return Number.isFinite(ms) ? Math.floor(ms / 1000) : null;
}

/** The earliest a change scheduled now may bind while anyone holds units. */
export function noticeFloor(nowSeconds: number): number {
  return nowSeconds + MIN_NOTICE_SECS;
}

/**
 * Whether the typed moment is one the plane will lift rather than honour: earlier than the
 * notice floor. Empty asks for the floor by name and is not "lifted"; unreadable is not a
 * moment at all.
 */
export function effectiveFromLifted(value: string, nowSeconds: number): boolean {
  const moment = effectiveFromSeconds(value);
  return moment !== null && moment !== 0 && moment < noticeFloor(nowSeconds);
}

/** A `datetime-local` value for a moment, in the operator's own zone — the inverse of
 *  `effectiveFromSeconds`, for the field's `min`/`max` and for tests that type a date. */
export function localDateTimeValue(at: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${at.getFullYear()}-${pad(at.getMonth() + 1)}-${pad(at.getDate())}T${pad(at.getHours())}:${pad(at.getMinutes())}`;
}

/** The request the plane receives. Callers check `draftProblem` first; the fallbacks here
 *  only restate for the types what that check has already proved. */
export function toRequest(service: string, draft: TermsDraft): ScheduleFeePolicyRequest {
  const terms = draftTerms(draft) ?? { management_bps: 0, performance_bps: 0, hurdle_bps: 0, basis: draft.basis, crystallization: draft.crystallization };
  return {
    service,
    ...terms,
    effective_from: effectiveFromSeconds(draft.effectiveFrom) ?? 0,
    reason: normalizeReason(draft.reason),
  };
}
