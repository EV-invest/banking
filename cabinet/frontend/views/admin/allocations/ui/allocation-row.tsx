"use client";

import { useLocale } from "@evinvest/i18n/react";

import type { AllocationWrite } from "@/entities/admin/api/admin-client";
import type { Allocation, AllocationAccessLevel } from "@/shared/contracts/admin";
import { ProductIcon } from "@/shared/ui/icons/products";
import { compactUnits } from "@/views/admin/lib/format";
import { backingOf } from "@/views/admin/allocations/lib/backing";
import type { AllocationPanelKind } from "@/views/admin/allocations/lib/panel";
import { AllocationAccessCell } from "@/views/admin/allocations/ui/allocation-access-cell";
import { AllocationEditor } from "@/views/admin/allocations/ui/allocation-editor";
import { AllocationRowActions } from "@/views/admin/allocations/ui/allocation-row-actions";
import { AllocationStateCell } from "@/views/admin/allocations/ui/allocation-state-cell";
import { BackingBadge } from "@/views/admin/allocations/ui/backing-badge";

/** One registry row's worth of data and callbacks — the same contract under both of its
 *  presentations, the table row here and the phone card in `AllocationCard`. */
export interface AllocationRowProps {
  row: Allocation;
  busy: boolean;
  editing: boolean;
  onEdit: () => void;
  onSave: (body: AllocationWrite) => void;
  onToggle: () => void;
  onSetAccess: (level: AllocationAccessLevel) => void;
  onOpenPanel: (kind: AllocationPanelKind) => void;
}

export function AllocationRow({ row, busy, editing, onEdit, onSave, onToggle, onSetAccess, onOpenPanel }: AllocationRowProps) {
  const locale = useLocale();
  // Same cache-age caveat as `icon`: `access` is optional on read for a persisted catalog
  // object older than this field. `view` is the hub's own default for an unset product.
  const access = row.access ?? "view";

  return (
    <>
      <tr>
        <td className="px-5 py-3">
          {/* The mark is shown in the row, not only inside the editor: it is what an
              investor sees in the rail, so an operator must be able to check it without
              opening a form that could then be saved by accident. */}
          <div className="flex items-center gap-2.5">
            <ProductIcon icon={row.icon} className="size-4 shrink-0 text-ink-soft" />
            <div className="min-w-0">
              <p className="font-medium">{row.title}</p>
              {row.summary && <p className="text-xs text-ink-soft">{row.summary}</p>}
            </div>
          </div>
        </td>
        <td className="px-5 py-3 font-mono-tech text-xs text-ink-soft">{row.service}</td>
        <td className="px-5 py-3">
          {/* The backing rides in the State cell: `in_kind` is a fact about how the product
              exits, like `closed` is, and it is the exception — `cash` draws nothing. */}
          <div className="flex flex-wrap items-center gap-1.5">
            <AllocationStateCell state={row.state} />
            <BackingBadge backing={backingOf(row)} />
          </div>
        </td>
        <td className="px-5 py-3">
          <AllocationAccessCell access={access} onChange={onSetAccess} />
        </td>
        {/* Read-only here: resizing the supply is a money decision and lives on the
            Valuation screen and the issuance panel's "pin cap", next to the issued figure
            it has to be judged against. */}
        <td className="px-5 py-3 tabular-nums text-ink-soft">{compactUnits(row.unit_cap, locale)}</td>
        <td className="px-5 py-3">
          <AllocationRowActions state={row.state} busy={busy} editing={editing} onOpenPanel={onOpenPanel} onEdit={onEdit} onToggle={onToggle} />
        </td>
      </tr>
      {editing && (
        <tr className="bg-ink/[0.03]">
          <td colSpan={6} className="px-5 py-4">
            <AllocationEditor row={row} busy={busy} onSave={onSave} />
          </td>
        </tr>
      )}
    </>
  );
}
