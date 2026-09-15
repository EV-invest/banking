"use client";

// The title row every side panel opens with: which product it belongs to, and the one
// control that dismisses it. Shared by the grants, issuance and book panels so the three
// cannot drift apart in how they name a row or where the close control sits.

import { X } from "lucide-react";

import { useT } from "@evinvest/i18n/react";

import type { Allocation } from "@/shared/contracts/admin";

export function PanelHeader({ allocation, onClose }: { allocation: Allocation; onClose: () => void }) {
  const t = useT();
  return (
    <div className="flex items-start justify-between gap-2">
      <div className="min-w-0">
        <p className="truncate font-semibold">{allocation.title}</p>
        <p className="truncate font-mono-tech text-xs text-ink-soft">{allocation.service}</p>
      </div>
      <button
        type="button"
        onClick={onClose}
        aria-label={t("ui.close")}
        className="rounded-md text-ink-soft outline-none transition-colors hover:text-ink focus-visible:ring-2 focus-visible:ring-ring"
      >
        <X className="size-4" />
      </button>
    </div>
  );
}
