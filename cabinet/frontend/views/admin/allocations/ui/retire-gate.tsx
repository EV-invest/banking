"use client";

// Why "Retire units" is off on a live product, and the one way past it. Retiring is for a
// product that has been wound down; on a draft or open one the hub refuses the burn unless
// the operator says `force` — so the override is a tick the operator makes on purpose,
// beside the reason, and never a default.

import { TriangleAlert } from "lucide-react";
import { useId } from "react";

import { useT } from "@evinvest/i18n/react";
import { Checkbox } from "@evinvest/uikit";

export function RetireGate({ force, onForce }: { force: boolean; onForce: (force: boolean) => void }) {
  const t = useT();
  const id = useId();
  return (
    <div className="space-y-2">
      <p className="text-xs text-ink-soft">{t("admin.alloc.retire.closeFirst")}</p>
      <div className="flex items-start gap-2">
        <Checkbox id={id} checked={force} onCheckedChange={onForce} className="mt-0.5" />
        <label htmlFor={id} className="text-xs">
          {t("admin.alloc.retire.force")}
        </label>
      </div>
      {force && (
        <p className="flex items-start gap-2 text-xs text-accent-warn">
          <TriangleAlert className="mt-0.5 size-3.5 shrink-0" /> {t("admin.alloc.retire.forceWarning")}
        </p>
      )}
    </div>
  );
}
