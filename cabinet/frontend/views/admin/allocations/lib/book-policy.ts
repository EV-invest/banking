// The book-terms form's model: the policy as the operator edits it, whether it can be
// sent, and the exact wire body it becomes. Pure and React-free, like `./issuance`.
//
// Rates are edited as percents and sent as basis points through `toBps` — the one
// conversion the cabinet allows (`shared/lib/rate.ts`). The three "advanced" fields are
// OPTIONAL on the wire: absent means "keep the hub's value", so an untouched field is left
// out of the body rather than sent as an empty string the hub would refuse.

import type { BookPolicy, SetBookPolicyBody } from "@/shared/contracts/book";

import { toBps, toPercentInput } from "../../../../shared/lib/rate.ts";

export interface BookPolicyDraft {
  open: boolean;
  /** Percent, as typed: "0.25". */
  takerFeePct: string;
  /** Decimal USDT; empty = leave as is. */
  tick: string;
  /** Decimal units; empty = leave as is. */
  lot: string;
  /** Percent; empty = leave as is. */
  slippagePct: string;
}

/** A decimal the hub spells "unset" as `"0"`, shown as the empty field it means. */
const orEmpty = (value: string | undefined): string => (Number(value ?? 0) > 0 ? (value ?? "") : "");

/** The form seeded from the policy as it stands — every field shown, so what is saved is
 *  what the operator read. An advanced term the hub has not set is an EMPTY field, not
 *  "0": empty is what the body leaves out, and leaving it out is what keeps it unset. */
export function bookPolicyDraft(policy: BookPolicy | null): BookPolicyDraft {
  return {
    open: policy?.book_open ?? false,
    takerFeePct: toPercentInput(policy?.taker_fee_bps ?? 0),
    tick: orEmpty(policy?.price_tick),
    lot: orEmpty(policy?.lot_size),
    slippagePct: policy?.market_slippage_bps ? toPercentInput(policy.market_slippage_bps) : "",
  };
}

/** Whether any advanced term is actually set. The hub spells "unset" as `"0"` for the
 *  decimals and `0` for the bps — both truthy as strings, neither a term. */
export function hasAdvancedTerms(policy: BookPolicy | null): boolean {
  if (!policy) return false;
  return Number(policy.price_tick ?? 0) > 0 || Number(policy.lot_size ?? 0) > 0 || (policy.market_slippage_bps ?? 0) > 0;
}

export type BookPolicyProblem = "fee" | "tick" | "lot" | "slippage";

const DECIMAL = /^\d+(\.\d+)?$/;
const MAX_BPS = 10_000;

const isDecimal = (raw: string): boolean => DECIMAL.test(raw.trim());

/** A rate field → bps, or `null` when it is not a percent within 0..100. */
function bps(raw: string): number | null {
  const value = toBps(raw);
  return value === null || value > MAX_BPS ? null : value;
}

export function bookPolicyProblem(draft: BookPolicyDraft): BookPolicyProblem | null {
  if (bps(draft.takerFeePct) === null) return "fee";
  if (draft.tick.trim() !== "" && !isDecimal(draft.tick)) return "tick";
  if (draft.lot.trim() !== "" && !isDecimal(draft.lot)) return "lot";
  if (draft.slippagePct.trim() !== "" && bps(draft.slippagePct) === null) return "slippage";
  return null;
}

export function setBookPolicyBody(service: string, draft: BookPolicyDraft): SetBookPolicyBody | null {
  if (bookPolicyProblem(draft) !== null) return null;
  const slippage = draft.slippagePct.trim() === "" ? undefined : (bps(draft.slippagePct) ?? undefined);
  return {
    service,
    book_open: draft.open,
    taker_fee_bps: bps(draft.takerFeePct) ?? 0,
    ...(draft.tick.trim() === "" ? {} : { price_tick: draft.tick.trim() }),
    ...(draft.lot.trim() === "" ? {} : { lot_size: draft.lot.trim() }),
    ...(slippage === undefined ? {} : { market_slippage_bps: slippage }),
  };
}
