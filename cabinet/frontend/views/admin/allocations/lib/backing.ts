// What stands behind a product's units, folded from the wire's optional field. Pure and
// React-free so the two badges and the operator's flip agree on one reading of an
// absent value.

// Relative and with the extension: the node test runner resolves no `@/` alias.
import type { Allocation, AllocationBacking } from "../../../../shared/contracts/admin.ts";

/** Absent reads as `cash` — the hub's own default for an unset product, and what a
 *  catalog object persisted before the field existed actually was. */
export function backingOf(allocation: Pick<Allocation, "backing">): AllocationBacking {
  return allocation.backing ?? "cash";
}

/** The one flip the operator can make from where the product stands. */
export function oppositeBacking(backing: AllocationBacking): AllocationBacking {
  return backing === "cash" ? "in_kind" : "cash";
}
