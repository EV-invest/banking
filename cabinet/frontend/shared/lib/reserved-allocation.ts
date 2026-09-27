// The reserved allocations (#245) — the platform's own money, hidden from the catalog:
// `fee` holds what the platform earned, `fund` its seed capital. Named in ONE place so the
// treasury, the cap table and the owners' room title them the same way, and so a
// `holder.kind === "allocation"` line is never looked up as a person.

import type { Translate } from "@evinvest/i18n";

const RESERVED_ALLOCATION_KEYS: Record<string, string> = {
  fee: "admin.holder.allocation.fee",
  fund: "admin.holder.allocation.fund",
};

/** A reserved allocation by name; one the hub reserves later falls back to its slug. */
export function reservedAllocationLabel(allocation: string, t: Translate): string {
  const key = RESERVED_ALLOCATION_KEYS[allocation];
  return key ? t(key) : allocation;
}
