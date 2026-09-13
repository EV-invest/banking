"use client";

// The registry row's Actions cell: open the access-grants or issuance panel, edit the
// product's presentation fields, or flip it open/closed. Split out of `AllocationRow` for
// the same reason `AllocationAccessCell` is — one cell per file keeps the row itself under
// the component-size ceiling as columns grow.

import { Coins, KeyRound, Loader2 } from "lucide-react";

import { useT } from "@evinvest/i18n/react";
import { Button } from "@evinvest/uikit";

import type { AllocationState } from "@/shared/contracts/admin";
import type { AllocationPanelKind } from "@/views/admin/allocations/lib/panel";

export function AllocationRowActions({
  state,
  busy,
  editing,
  onOpenPanel,
  onEdit,
  onToggle,
}: {
  state: AllocationState;
  busy: boolean;
  editing: boolean;
  onOpenPanel: (kind: AllocationPanelKind) => void;
  onEdit: () => void;
  onToggle: () => void;
}) {
  const t = useT();
  return (
    // i18n-max: 12 per verb — four shrink-0 controls share this cell.
    <div className="flex justify-end gap-2">
      <Button type="button" variant="outline" size="sm" disabled={busy} onClick={() => onOpenPanel("grants")}>
        <KeyRound className="size-3.5" />
        {t("admin.alloc.grants.action")}
      </Button>
      <Button type="button" variant="outline" size="sm" disabled={busy} onClick={() => onOpenPanel("issue")}>
        <Coins className="size-3.5" />
        {t("admin.alloc.issue.action")}
      </Button>
      <Button type="button" variant="outline" size="sm" disabled={busy} onClick={onEdit}>
        {editing ? t("ui.cancel") : t("ui.edit")}
      </Button>
      <Button type="button" variant="outline" size="sm" disabled={busy} onClick={onToggle}>
        {busy ? <Loader2 className="size-4 animate-spin" /> : state === "open" ? t("admin.alloc.close") : t("admin.alloc.open")}
      </Button>
    </div>
  );
}
