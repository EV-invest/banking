"use client";

// The slot beside the registry table, and which panel fills it. Same collapse-width
// idiom as the Users screen's row drawer — see `UserDrawer` in
// `views/admin/users/ui/users-view.tsx` for why the width is fixed rather than `w-full`,
// and why the presence boundary lives at this level rather than inside each panel.

import { Panel, PanelPresence, PanelSwap } from "@/shared/ui/motion";
import type { OpenAllocationPanel } from "@/views/admin/allocations/lib/panel";
import { GrantsPanel } from "@/views/admin/allocations/ui/grants-panel";
import { IssuancePanel } from "@/views/admin/allocations/ui/issuance-panel";

export function AllocationSidePanel({ panel, onClose }: { panel: OpenAllocationPanel | null; onClose: () => void }) {
  return (
    <PanelPresence>
      {panel && (
        <Panel key="side-panel" collapse={{ gap: "1.5rem", width: "21.25rem" }} className="shrink-0 self-start overflow-hidden">
          {/* Keyed on kind AND row: switching from grants to issuance on the same row is a
              swap too, not an in-place re-render of a panel that is no longer there. */}
          <PanelSwap swapKey={`${panel.kind}:${panel.row.service}`}>
            {panel.kind === "grants" ? (
              <GrantsPanel key={panel.row.service} allocation={panel.row} onClose={onClose} />
            ) : (
              <IssuancePanel key={panel.row.service} allocation={panel.row} onClose={onClose} />
            )}
          </PanelSwap>
        </Panel>
      )}
    </PanelPresence>
  );
}
