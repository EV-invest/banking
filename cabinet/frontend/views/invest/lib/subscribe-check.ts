// What the subscribe form can tell before the click. The hub is authoritative on every
// one of these refusals; stating them here turns "Subscription failed" after the submit
// into a sentence, and a way out, before it (#396).

import type { FundNav } from "@/shared/contracts";

// Relative and with the extension, like `./product.ts`: the node test runner resolves no
// `@/` alias, and `./format` only re-exports this module.
import { toBaseUnits } from "../../../shared/lib/money.ts";
import { unitsForCash } from "./product.ts";

/**
 * Why the amount as typed cannot go through, in the order the reader can act on it:
 * money they do not have comes first (the fix is a top-up, not a smaller number), then
 * the fund's own ceiling, then an amount too small to mint a single unit.
 */
export type SubscribeIssue = "insufficient" | "overCap" | "dust";

export interface SubscribeCheck {
  /** Units the amount buys, floored as the hub floors; `null` while there is no amount or no mark. */
  preview: bigint | null;
  /** Units left under the cap — `remaining_capacity` already nets off in-flight mints. */
  headroom: bigint | null;
  issue: SubscribeIssue | null;
}

/**
 * `available` is `null` when the wallet has not been read (or the read failed): a balance
 * nobody could read is not a balance of zero, so nothing is gated on it and the hub's own
 * refusal still stops the submit — the same rule `features/kyc/lib/money-gate` applies to
 * an unread tier.
 */
export function checkSubscribe({ amount, available, nav }: { amount: string; available: string | null; nav: FundNav | null }): SubscribeCheck {
  const cash = toBaseUnits(amount);
  const preview = unitsForCash(amount, nav?.nav);
  const headroom = nav ? toBaseUnits(nav.remaining_capacity) : null;

  const insufficient = available !== null && cash > 0n && cash > toBaseUnits(available);
  const overCap = preview !== null && headroom !== null && preview > headroom;
  const dust = cash > 0n && preview === 0n;

  return { preview, headroom, issue: insufficient ? "insufficient" : overCap ? "overCap" : dust ? "dust" : null };
}

/** Whether the form may submit: a positive preview and no stated reason against it. */
export function canSubmit(check: SubscribeCheck): boolean {
  return check.preview !== null && check.preview > 0n && check.issue === null;
}
