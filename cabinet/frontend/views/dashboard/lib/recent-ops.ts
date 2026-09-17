// The rows of the Recent operations card, shaped from the timeline the hub returns.

import type { Locale, Translate } from "@evinvest/i18n";
import type { LucideIcon } from "lucide-react";

import type { Operation } from "@/shared/contracts";
import { DASH_ADDRESS, formatUsd, shortAddress } from "@/views/dashboard/lib/format";
import { amountTone, kindBadge, kindLabel, kindMeta, networkLabel, stateLabel } from "@/views/operations/lib/format";

export interface Op {
  id: string;
  /** The kind's mark; `null` for a kind this build has no mark for, which falls back to
   *  {@link tag}. Same rule as the operations timeline — the vocabulary is shared. */
  icon: LucideIcon | null;
  /** The text a mark-less kind wears instead. */
  tag: string;
  tagClass: string;
  title: string;
  sub: string;
  amount: string;
  amountClass: string;
}

// One timeline row rendered in the dashboard's summary-money policy (`formatUsd`, to the
// cent with a currency symbol) rather than the ledger policy the Operations page uses —
// same data, different unit of measure for the surface it sits on. The badge, tone and
// sign vocabulary is shared with `/operations` so a row reads identically in both places.
export function toOp(operation: Operation, index: number, titleOf: (service: string | undefined) => string, t: Translate, locale: Locale): Op {
  const meta = kindMeta(operation.kind);
  const sign = meta.direction === "in" ? "+" : meta.direction === "out" ? "−" : "";
  return {
    id: `${operation.kind ?? ""}-${operation.id ?? ""}-${index}`,
    icon: meta.icon,
    tag: kindBadge(operation.kind),
    tagClass: meta.tone,
    title: opTitle(operation, titleOf, t),
    sub: opSub(operation, t),
    // A queued redemption is not yet priced, so it shows the units it reserved — a
    // formatted zero would claim the user was paid nothing.
    amount: operation.amount ? `${sign}${formatUsd(operation.amount, locale)}` : t("dash.unitsAmount", { n: Number(operation.units ?? 0), units: operation.units ?? "0" }),
    amountClass: operation.amount ? amountTone(meta.direction) : "text-ink-soft",
  };
}

function opTitle(operation: Operation, titleOf: (service: string | undefined) => string, t: Translate): string {
  if (operation.kind === "subscription") return t("dash.op.subscribed", { fund: titleOf(operation.service) });
  if (operation.kind === "redemption") return t("dash.op.redeemed", { fund: titleOf(operation.service) });
  if (operation.kind === "fee") return t("ops.op.feeCharged", { fund: titleOf(operation.service) });
  if (operation.kind === "withdrawal") return t("dash.op.withdrawal", { network: networkLabel(operation.network) });
  if (operation.kind === "deposit") return t("dash.op.deposit", { network: networkLabel(operation.network) });
  return kindLabel(operation.kind, t);
}

function opSub(operation: Operation, t: Translate): string {
  // The lifecycle state reaches the reader through the same vocabulary the operations
  // timeline uses — it used to be the bare wire identifier, English by construction.
  const state = stateLabel(operation.state, t);
  if (operation.kind === "withdrawal") return t("dash.op.sub", { ref: shortAddress(operation.address, DASH_ADDRESS), state });
  if (operation.kind === "deposit") return t("dash.op.sub", { ref: shortAddress(operation.tx_ref, DASH_ADDRESS), state });
  return state;
}
