"use client";

// The registry row's Actions cell: open the grants, issuance or book panel, edit the
// product's presentation fields, or flip it open/closed. Split out of `AllocationRow` for
// the same reason `AllocationAccessCell` is — one cell per file keeps the row itself under
// the component-size ceiling as columns grow.

import { ChartCandlestick, Coins, KeyRound } from "lucide-react";

import { useT } from "@evinvest/i18n/react";
import { Button, Spinner } from "@evinvest/uikit";

import type { AllocationState } from "@/shared/contracts/admin";
import { cn } from "@/shared/lib/cn";
import type { AllocationPanelKind } from "@/views/admin/allocations/lib/panel";

export function AllocationRowActions({
  state,
  busy,
  editing,
  onOpenPanel,
  onEdit,
  onToggle,
  className,
}: {
  state: AllocationState;
  busy: boolean;
  editing: boolean;
  onOpenPanel: (kind: AllocationPanelKind) => void;
  onEdit: () => void;
  onToggle: () => void;
  className?: string;
}) {
  const t = useT();
  return (
    // i18n-max: 12 per verb. Five shrink-0 controls share this cell, in two groups: the
    // three panel openers, then the two row verbs. Each group wraps rather than overflows —
    // with the side panel open the cell is narrower than five in a row — and the outer
    // wrap breaks between the groups first, so the line break lands at the semantic seam
    // rather than wherever the translation happened to put it.
    <div className={cn("flex flex-wrap justify-end gap-2", className)}>
      <div className="flex flex-wrap gap-2">
        <Button type="button" variant="outline" size="sm" disabled={busy} onClick={() => onOpenPanel("grants")}>
          <KeyRound className="size-3.5" />
          {t("admin.alloc.grants.action")}
        </Button>
        <Button type="button" variant="outline" size="sm" disabled={busy} onClick={() => onOpenPanel("issue")}>
          <Coins className="size-3.5" />
          {t("admin.alloc.issue.action")}
        </Button>
        <Button type="button" variant="outline" size="sm" disabled={busy} onClick={() => onOpenPanel("book")}>
          <ChartCandlestick className="size-3.5" />
          {t("admin.alloc.book.action")}
        </Button>
      </div>
      <div className="flex flex-wrap gap-2">
        <Button type="button" variant="outline" size="sm" disabled={busy} onClick={onEdit}>
          {editing ? t("ui.cancel") : t("ui.edit")}
        </Button>
        <Button type="button" variant="outline" size="sm" disabled={busy} onClick={onToggle}>
          {busy ? <Spinner aria-hidden /> : state === "open" ? t("admin.alloc.close") : t("admin.alloc.open")}
        </Button>
      </div>
    </div>
  );
}
