// The side panels a registry row can open beside the table. One vocabulary shared by the
// row's action cell (which asks for one) and the view (which shows one), so adding a
// panel is a new member here rather than a new callback threaded through both.

import type { Allocation } from "@/shared/contracts/admin";

export type AllocationPanelKind = "grants" | "issue";

/** The one side panel open beside the table — a row and which of its panels. One slot,
 *  not one per kind: the panel is where the operator is working, and two open at once
 *  would be two rows claiming the same attention. */
export interface OpenAllocationPanel {
  kind: AllocationPanelKind;
  row: Allocation;
}
