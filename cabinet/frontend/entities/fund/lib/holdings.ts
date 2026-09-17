import type { PositionList } from "@/shared/contracts";

/**
 * Whether the account holds any units at all — `null` when the positions have not been
 * read, which is a different answer from "none". A comparison, not money math: the wire
 * string is only asked whether it is above zero.
 */
export function hasHoldings(list: PositionList | undefined): boolean | null {
  if (list === undefined) return null;
  return (list.positions ?? []).some((position) => Number(position.units ?? "0") > 0);
}
