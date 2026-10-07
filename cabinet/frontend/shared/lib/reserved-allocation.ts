// The reserved allocations (#245) — the platform's own money, hidden from the catalog:
// `fee` holds what the platform earned, `fund` its seed capital. Named in ONE place so the
// treasury, the cap table and the owners' room title them the same way, and so a
// `holder.kind === "allocation"` line is never looked up as a person.

import type { Translate } from "@evinvest/i18n";

import { wordFor } from "./wire-words.ts";
const reservedAllocationWords = (t: Translate): Readonly<Record<string, string>> => ({
  fee: t("admin.holder.allocation.fee", "Platform fees"),
  fund: t("admin.holder.allocation.fund", "Platform capital"),
});

/** A reserved allocation by name; one the hub reserves later falls back to its slug. */
export function reservedAllocationLabel(allocation: string, t: Translate): string {
  return wordFor(reservedAllocationWords(t), allocation) ?? allocation;
}
