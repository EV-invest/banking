"use client";

// A registry row on a phone: the same cells as `AllocationRow`, stacked. Below `md` the
// six-column table only fits by scrolling sideways, which hides the one column an
// operator came for — the actions — so the row becomes a card and the actions a wrapping
// button row that is always in view.

import { useT } from "@evinvest/i18n/react";

import { ProductIcon } from "@/shared/ui/icons/products";
import { compactUnits } from "@/views/admin/lib/format";
import { backingOf } from "@/views/admin/allocations/lib/backing";
import { AllocationAccessCell } from "@/views/admin/allocations/ui/allocation-access-cell";
import { AllocationEditor } from "@/views/admin/allocations/ui/allocation-editor";
import type { AllocationRowProps } from "@/views/admin/allocations/ui/allocation-row";
import { AllocationRowActions } from "@/views/admin/allocations/ui/allocation-row-actions";
import { AllocationStateCell } from "@/views/admin/allocations/ui/allocation-state-cell";
import { BackingBadge } from "@/views/admin/allocations/ui/backing-badge";

export function AllocationCard({ row, busy, editing, onEdit, onSave, onToggle, onSetAccess, onOpenPanel }: AllocationRowProps) {
  const t = useT();
  // Same cache-age caveat as in `AllocationRow`: `view` is the hub's default for an unset product.
  const access = row.access ?? "view";

  return (
    <div className="space-y-3 px-4 py-4">
      <div className="flex items-start gap-2.5">
        <ProductIcon icon={row.icon} className="mt-0.5 size-4 shrink-0 text-ink-soft" />
        <div className="min-w-0 flex-1">
          <p className="font-medium">{row.title}</p>
          {row.summary && <p className="text-xs text-ink-soft">{row.summary}</p>}
          <p className="font-mono-tech text-xs text-ink-soft">{row.service}</p>
        </div>
        <div className="flex shrink-0 flex-col items-end gap-1">
          <AllocationStateCell state={row.state} />
          <BackingBadge backing={backingOf(row)} />
        </div>
      </div>
      <dl className="space-y-2 text-sm">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <dt className="text-xs text-ink-soft">{t("admin.alloc.col.access")}</dt>
          <dd>
            <AllocationAccessCell access={access} onChange={onSetAccess} />
          </dd>
        </div>
        <div className="flex items-center justify-between gap-2">
          <dt className="text-xs text-ink-soft">{t("admin.alloc.col.unitCap")}</dt>
          <dd className="tabular-nums text-ink-soft">{compactUnits(row.unit_cap)}</dd>
        </div>
      </dl>
      <AllocationRowActions state={row.state} busy={busy} editing={editing} onOpenPanel={onOpenPanel} onEdit={onEdit} onToggle={onToggle} className="justify-start" />
      {editing && (
        <div className="rounded-lg bg-ink/[0.03] p-3">
          <AllocationEditor row={row} busy={busy} onSave={onSave} />
        </div>
      )}
    </div>
  );
}
