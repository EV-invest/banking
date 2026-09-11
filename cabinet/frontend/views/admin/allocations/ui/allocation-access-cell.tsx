"use client";

// The registry row's Access cell: the product's current default as a chip, plus the
// inline switcher that writes it. Its own file so `AllocationRow` reads as one row's
// worth of cells rather than growing past the component-size ceiling every time a column
// gains a control.

import { useT } from "@evinvest/i18n/react";

import type { AllocationAccessLevel } from "@/shared/contracts/admin";
import { cn } from "@/shared/lib/cn";
import { accessLabel, accessTone } from "@/views/admin/allocations/lib/access";
import { AccessSelect } from "@/views/admin/allocations/ui/pickers";

export function AllocationAccessCell({ access, onChange }: { access: AllocationAccessLevel; onChange: (level: AllocationAccessLevel) => void }) {
  const t = useT();
  return (
    <div className="flex items-center gap-2">
      <span className={cn("inline-flex shrink-0 items-center whitespace-nowrap rounded-full border px-2 py-0.5 text-xs font-medium", accessTone(access))}>{accessLabel(access, t)}</span>
      <AccessSelect value={access} onChange={onChange} className="w-32" />
    </div>
  );
}
