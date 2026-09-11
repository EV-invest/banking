"use client";

import { useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Button, Input } from "@evinvest/uikit";

import type { AllocationWrite } from "@/entities/admin/api/admin-client";
import type { Allocation, AllocationAccessLevel, AllocationIcon } from "@/shared/contracts/admin";
import { cn } from "@/shared/lib/cn";
import { ProductIcon } from "@/shared/ui/icons/products";
import { compactUnits } from "@/views/admin/lib/format";
import { AllocationAccessCell } from "@/views/admin/allocations/ui/allocation-access-cell";
import { AllocationRowActions } from "@/views/admin/allocations/ui/allocation-row-actions";
import { AllocationStateCell } from "@/views/admin/allocations/ui/allocation-state-cell";
import { IconSelect } from "@/views/admin/allocations/ui/pickers";

const TEAL_CTA = "bg-main-accent-t1 text-main-black hover:bg-main-accent-t1/90";

export function AllocationRow({
  row,
  busy,
  editing,
  onEdit,
  onSave,
  onToggle,
  onSetAccess,
  onOpenGrants,
}: {
  row: Allocation;
  busy: boolean;
  editing: boolean;
  onEdit: () => void;
  onSave: (body: AllocationWrite) => void;
  onToggle: () => void;
  onSetAccess: (level: AllocationAccessLevel) => void;
  onOpenGrants: () => void;
}) {
  const t = useT();
  const [title, setTitle] = useState(row.title);
  const [summary, setSummary] = useState(row.summary);
  // Seeded from the row so the picker opens on the icon the product already wears, and a
  // rename submits it back unchanged. `?? "fund"` covers the row arriving from a cache
  // written before the field existed.
  const [icon, setIcon] = useState<AllocationIcon>(row.icon ?? "fund");
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
            <ProductIcon icon={row.icon} className="size-4 shrink-0 text-muted-foreground" />
            <div className="min-w-0">
              <p className="font-medium">{row.title}</p>
              {row.summary && <p className="text-xs text-muted-foreground">{row.summary}</p>}
            </div>
          </div>
        </td>
        <td className="px-5 py-3 font-mono-tech text-xs text-muted-foreground">{row.service}</td>
        <td className="px-5 py-3">
          <AllocationStateCell state={row.state} />
        </td>
        <td className="px-5 py-3">
          <AllocationAccessCell access={access} onChange={onSetAccess} />
        </td>
        {/* Read-only here: resizing the supply is a money decision and lives on the
            Valuation screen, next to the issued figure it has to be judged against. */}
        <td className="px-5 py-3 tabular-nums text-muted-foreground">{compactUnits(row.unit_cap)}</td>
        <td className="px-5 py-3">
          <AllocationRowActions state={row.state} busy={busy} editing={editing} onOpenGrants={onOpenGrants} onEdit={onEdit} onToggle={onToggle} />
        </td>
      </tr>
      {editing && (
        <tr className="bg-foreground/[0.03]">
          <td colSpan={6} className="px-5 py-4">
            <div className="flex flex-wrap items-end gap-3">
              {/* `flex flex-col`, not `block` + `space-y`: the uikit Input is `inline-flex`,
                  so a narrow one shares the line with its label unless the column is
                  explicit. */}
              <label className="flex w-56 flex-col gap-1.5">
                <span className="text-xs text-muted-foreground">{t("admin.alloc.field.title")}</span>
                <Input value={title} onChange={(e) => setTitle(e.target.value)} className="w-full" />
              </label>
              <label className="flex min-w-56 flex-1 flex-col gap-1.5">
                <span className="text-xs text-muted-foreground">{t("admin.alloc.field.summary")}</span>
                <Input value={summary} onChange={(e) => setSummary(e.target.value)} placeholder={t("admin.alloc.placeholder.summary")} className="w-full" />
              </label>
              {/* Not a `<label>`: the trigger is a button, and wrapping it would make the
                  caption a second click target that reopens the popup it just closed. */}
              <div className="flex w-44 flex-col gap-1.5">
                <span className="text-xs text-muted-foreground">{t("admin.alloc.field.icon")}</span>
                <IconSelect value={icon} onChange={setIcon} />
              </div>
              <Button type="button" className={cn(TEAL_CTA)} disabled={busy || !title.trim()} onClick={() => onSave({ service: row.service, title, summary, icon })}>
                {t("ui.save")}
              </Button>
            </div>
          </td>
        </tr>
      )}
    </>
  );
}
