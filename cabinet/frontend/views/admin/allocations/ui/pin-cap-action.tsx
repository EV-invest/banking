"use client";

// The Holders section's one write: close the cap at exactly what is out. Two clicks,
// because it is a money policy — the first shows the exact move ("cap 100M → 16,250.00"),
// the second sends it. The rules that disable it — including "wait, a mint is still in
// the relay" — live in `lib/pin-cap.ts`, tested.

import { Loader2, Pin, TriangleAlert } from "lucide-react";
import { useState } from "react";

import type { Translate } from "@evinvest/i18n";
import { useT } from "@evinvest/i18n/react";
import { Button } from "@evinvest/uikit";

import { setAllocationUnitCap } from "@/entities/admin/api/admin-client";
import type { Allocation, UnitHolders } from "@/shared/contracts/admin";
import { errorMessage } from "@/shared/lib/api-client";
import { TAG } from "@/shared/lib/cache-tags";
import { cn } from "@/shared/lib/cn";
import { compactUnits, formatUnits } from "@/shared/lib/money";
import { revalidateTag } from "@/shared/lib/resource";
import { pinCapVerdict, type PinCapVerdict } from "@/views/admin/allocations/lib/pin-cap";

const TEAL_CTA = "bg-main-accent-t1 text-main-black hover:bg-main-accent-t1/90";

export function PinCapAction({ allocation, holders }: { allocation: Allocation; holders: UnitHolders }) {
  const t = useT();
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const verdict = pinCapVerdict(allocation.unit_cap, holders.units_outstanding, holders.queued_units);

  const pin = async () => {
    if (verdict.kind !== "pinnable") return;
    setBusy(true);
    setError(null);
    try {
      await setAllocationUnitCap(allocation.service, verdict.to);
      // The cap lives on the allocation and is echoed on the mark (`unit_cap`,
      // `remaining_capacity`) and in the investor catalog — the same three the Valuation
      // screen names after its own cap write. The registry refresh is what hands this
      // panel the new figure, so the button reads "already pinned" without a reopen.
      revalidateTag(TAG.adminAllocations, TAG.nav, TAG.catalog);
      setConfirming(false);
    } catch (e) {
      setError(errorMessage(e, t));
    } finally {
      setBusy(false);
    }
  };

  const hint = disabledReason(verdict, t);

  return (
    <div className="space-y-2">
      {confirming && verdict.kind === "pinnable" ? (
        <div className="space-y-2 rounded-lg border border-border bg-main-surface p-3">
          <p className="text-xs tabular-nums">{t("admin.alloc.pinCap.confirm", { from: compactUnits(verdict.from), to: formatUnits(verdict.to) })}</p>
          <div className="flex gap-2">
            <Button type="button" variant="outline" size="sm" className="flex-1" disabled={busy} onClick={() => setConfirming(false)}>
              {t("ui.cancel")}
            </Button>
            <Button type="button" size="sm" className={cn("flex-1", TEAL_CTA)} disabled={busy} onClick={pin}>
              {busy ? <Loader2 className="size-3.5 animate-spin" /> : null}
              {t("admin.alloc.pinCap.submit")}
            </Button>
          </div>
        </div>
      ) : (
        <Button type="button" variant="outline" size="sm" className="w-full" disabled={verdict.kind !== "pinnable"} onClick={() => setConfirming(true)}>
          <Pin className="size-3.5" />
          {t("admin.alloc.pinCap.action")}
        </Button>
      )}
      {hint && <p className="text-xs text-ink-soft">{hint}</p>}
      {error && (
        <p className="flex items-center gap-2 text-xs text-destructive">
          <TriangleAlert className="size-3.5" /> {error}
        </p>
      )}
    </div>
  );
}

/** Why the button is greyed out — one sentence per non-pinnable verdict, none otherwise. */
function disabledReason(verdict: PinCapVerdict, t: Translate): string | null {
  switch (verdict.kind) {
    case "queuedPending":
      return t("admin.alloc.pinCap.queued", { units: formatUnits(verdict.queued) });
    case "nothingIssued":
      return t("admin.alloc.pinCap.nothingIssued");
    case "alreadyPinned":
      return t("admin.alloc.pinCap.alreadyPinned");
    case "pinnable":
      return null;
  }
}
