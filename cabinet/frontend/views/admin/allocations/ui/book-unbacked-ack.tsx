"use client";

// The operator's acknowledgement that units may trade for cash the fund does not hold.
// Shown on every book form, since a product's backing can flip after its book opened;
// the warning speaks louder — amber, like the `in_kind` chip — when it applies right now,
// and the form names the missing tick as the reason it will not submit instead of
// relaying the hub's 412 afterwards.

import { TriangleAlert } from "lucide-react";
import { useId } from "react";

import { useT } from "@evinvest/i18n/react";
import { Badge, Checkbox } from "@evinvest/uikit";

import type { AllocationBacking } from "@/shared/contracts/admin";
import { cn } from "@/shared/lib/cn";

export function BookUnbackedAck({ checked, onChange, backing, required, disabled }: { checked: boolean; onChange: (checked: boolean) => void; backing: AllocationBacking; required: boolean; disabled: boolean }) {
  const t = useT();
  const id = useId();
  const inKind = backing === "in_kind";
  return (
    <div className="space-y-2">
      <div className="flex items-start gap-2">
        <Checkbox id={id} checked={checked} onCheckedChange={onChange} disabled={disabled} className="mt-0.5" />
        <label htmlFor={id} className="text-xs">
          {t("admin.alloc.book.unbacked.ack")}
        </label>
      </div>
      <p className={cn("flex items-start gap-2 text-xs", inKind ? "text-main-accent-t3" : "text-ink-soft")}>
        <TriangleAlert className="mt-0.5 size-3.5 shrink-0" /> {t("admin.alloc.book.unbacked.warning")}
      </p>
      {required && <p className="text-xs text-accent-error">{t("admin.alloc.book.unbacked.required")}</p>}
    </div>
  );
}

/** The saved state of the acknowledgement, as a chip in the panel's summary. Nothing
 *  when it was never given — the common case should stay quiet. */
export function UnbackedAckBadge({ acknowledged }: { acknowledged: boolean }) {
  const t = useT();
  if (!acknowledged) return null;
  return (
    // i18n-max: 32 — a chip above the form.
    <Badge variant="outline" className="whitespace-nowrap border-main-accent-t3/40 text-main-accent-t3" title={t("admin.alloc.book.unbacked.warning")}>
      {t("admin.alloc.book.unbacked.acknowledged")}
    </Badge>
  );
}
