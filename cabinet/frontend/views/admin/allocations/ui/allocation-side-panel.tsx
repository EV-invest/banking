"use client";

// The slot beside the registry table, and which panel fills it. Same collapse-width
// idiom as the Users screen's row drawer — see `UserDrawer` in
// `views/admin/users/ui/users-view.tsx` for why the width is fixed rather than `w-full`,
// and why the presence boundary lives at this level rather than inside each panel.
//
// Below `lg` there is no room beside the table for a 340px column, so the same panel is
// presented over the content as a bottom sheet instead — the operations timeline makes
// the identical Popover → Drawer trade at the same breakpoint.

import { Drawer, DrawerContent, DrawerTitle } from "@evinvest/uikit";

import { cn } from "@/shared/lib/cn";
import { useIsCompact } from "@/shared/lib/use-is-compact";
import { Panel, PanelPresence, PanelSwap } from "@/shared/ui/motion";
import type { OpenAllocationPanel } from "@/views/admin/allocations/lib/panel";
import { GrantsPanel } from "@/views/admin/allocations/ui/grants-panel";
import { IssuancePanel } from "@/views/admin/allocations/ui/issuance-panel";

// Inside the sheet the panel is the sheet: full width, and the card's own frame would be
// a border inside a border.
const SHEET_PANEL = "w-full rounded-none border-0 shadow-none";

function PanelBody({ panel, onClose, className }: { panel: OpenAllocationPanel; onClose: () => void; className?: string }) {
  return panel.kind === "grants" ? (
    <GrantsPanel key={panel.row.service} allocation={panel.row} onClose={onClose} className={className} />
  ) : (
    <IssuancePanel key={panel.row.service} allocation={panel.row} onClose={onClose} className={className} />
  );
}

export function AllocationSidePanel({ panel, onClose }: { panel: OpenAllocationPanel | null; onClose: () => void }) {
  const compact = useIsCompact("lg");

  if (compact) {
    return (
      <Drawer open={panel !== null} onOpenChange={(open) => !open && onClose()}>
        {/* UIKIT-MIRROR: drawer-animation — TEMPORARY, mirrors EV-invest/lib#96.
            The published uikit's Drawer ships with no transition at all, so the sheet
            appears in a single frame. These are the kit's own classes, passed here until
            the npm bump carries them; `shared/config/uikit-mirror.test.ts` fails the
            moment the installed uikit animates, which is the signal to delete this. */}
        <DrawerContent
          className={cn(
            "data-[state=open]:animate-in data-[state=open]:slide-in-from-bottom",
            "transition duration-500 ease-in-out",
            "max-h-[85vh] overflow-y-auto",
          )}
        >
          {panel && (
            <>
              <DrawerTitle className="sr-only">{panel.row.title}</DrawerTitle>
              <PanelBody panel={panel} onClose={onClose} className={SHEET_PANEL} />
            </>
          )}
        </DrawerContent>
      </Drawer>
    );
  }

  return (
    <PanelPresence>
      {panel && (
        <Panel key="side-panel" collapse={{ gap: "1.5rem", width: "21.25rem" }} className="shrink-0 self-start overflow-hidden">
          {/* Keyed on kind AND row: switching from grants to issuance on the same row is a
              swap too, not an in-place re-render of a panel that is no longer there. */}
          <PanelSwap swapKey={`${panel.kind}:${panel.row.service}`}>
            <PanelBody panel={panel} onClose={onClose} />
          </PanelSwap>
        </Panel>
      )}
    </PanelPresence>
  );
}
