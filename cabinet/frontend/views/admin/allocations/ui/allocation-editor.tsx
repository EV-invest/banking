"use client";

// The inline editor for a product's presentation fields — title, summary, icon. One
// component under both presentations of a registry row (the table row's expansion and
// the phone card's foot), so the two cannot disagree on what is editable. It mounts only
// while editing, so its fields seed from the row as it stands when the editor opens and
// a cancel-then-reopen never shows stale typing.

import { useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Button, Input } from "@evinvest/uikit";

import type { AllocationWrite } from "@/entities/admin/api/admin-client";
import type { Allocation, AllocationIcon } from "@/shared/contracts/admin";
import { cn } from "@/shared/lib/cn";
import { IconSelect } from "@/views/admin/allocations/ui/pickers";

const TEAL_CTA = "bg-primary text-on-primary hover:bg-primary/90";

export function AllocationEditor({ row, busy, onSave }: { row: Allocation; busy: boolean; onSave: (body: AllocationWrite) => void }) {
  const t = useT();
  const [title, setTitle] = useState(row.title);
  const [summary, setSummary] = useState(row.summary);
  // `?? "fund"` covers the row arriving from a cache written before the field existed,
  // and a rename then submits the mark back unchanged.
  const [icon, setIcon] = useState<AllocationIcon>(row.icon ?? "fund");

  return (
    <div className="flex flex-wrap items-end gap-3">
      {/* `flex flex-col`, not `block` + `space-y`: the uikit Input is `inline-flex`, so a
          narrow one shares the line with its label unless the column is explicit. */}
      <label className="flex w-56 max-w-full flex-col gap-1.5">
        <span className="text-xs text-ink-soft">{t("admin.alloc.field.title")}</span>
        <Input value={title} onChange={(e) => setTitle(e.target.value)} className="w-full" />
      </label>
      <label className="flex min-w-56 max-w-full flex-1 flex-col gap-1.5">
        <span className="text-xs text-ink-soft">{t("admin.alloc.field.summary")}</span>
        <Input value={summary} onChange={(e) => setSummary(e.target.value)} placeholder={t("admin.alloc.placeholder.summary")} className="w-full" />
      </label>
      {/* Not a `<label>`: the trigger is a button, and wrapping it would make the caption
          a second click target that reopens the popup it just closed. */}
      <div className="flex w-44 max-w-full flex-col gap-1.5">
        <span className="text-xs text-ink-soft">{t("admin.alloc.field.icon")}</span>
        <IconSelect value={icon} onChange={setIcon} />
      </div>
      <Button type="button" className={cn(TEAL_CTA)} disabled={busy || !title.trim()} onClick={() => onSave({ service: row.service, title, summary, icon })}>
        {t("ui.save")}
      </Button>
    </div>
  );
}
