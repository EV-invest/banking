"use client";

// The registry row's Actions cell: open the access-grants or issuance panel, edit the
// product's presentation fields, or flip it open/closed. Split out of `AllocationRow` for
// the same reason `AllocationAccessCell` is — one cell per file keeps the row itself under
// the component-size ceiling as columns grow.

import { ChartCandlestick, Coins, KeyRound, Loader2 } from "lucide-react";

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
    // i18n-max: 12 per verb. Two rows on purpose: the three panel openers above the two
    // row verbs. Five in one row ran past the card's edge whenever a side panel was open,
    // and a plain wrap put the break wherever the translation happened to land it.
    <div className="flex flex-col items-end gap-2">
      <div className="flex gap-2">
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
      <div className="flex gap-2">
        <Button type="button" variant="outline" size="sm" disabled={busy} onClick={onEdit}>
          {editing ? t("ui.cancel") : t("ui.edit")}
        </Button>
        <Button type="button" variant="outline" size="sm" disabled={busy} onClick={onToggle}>
          {busy ? <Loader2 className="size-4 animate-spin" /> : state === "open" ? t("admin.alloc.close") : t("admin.alloc.open")}
        </Button>
      </div>
    </div>
  );
}
