"use client";

// The issuance panel's statement of what stands behind the units, and the operator's
// flip. Two clicks, like the cap pin, because it decides whether `Redeem` pays holders
// out of the fund's cash or refuses them to the book — a policy, not a label. The hub
// sets `in_kind` by itself on the first in-kind mint; this is the only way back.

import { Loader2, RefreshCw, TriangleAlert } from "lucide-react";
import { useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Button } from "@evinvest/uikit";

import { setAllocationBacking } from "@/entities/admin/api/admin-client";
import type { Allocation } from "@/shared/contracts/admin";
import { errorMessage } from "@/shared/lib/api-client";
import { TAG } from "@/shared/lib/cache-tags";
import { revalidateTag } from "@/shared/lib/resource";
import { backingOf, oppositeBacking } from "@/views/admin/allocations/lib/backing";
import { BackingBadge, backingHintKey } from "@/views/admin/allocations/ui/backing-badge";

export function BackingAction({ allocation }: { allocation: Allocation }) {
  const t = useT();
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const backing = backingOf(allocation);
  const next = oppositeBacking(backing);

  const flip = async () => {
    setBusy(true);
    setError(null);
    try {
      await setAllocationBacking(allocation.service, next);
      // The field lives on the allocation, which both the registry and the investor's
      // catalog and detail read — the registry refresh is what hands this panel the new
      // value, so the badge flips without a reopen.
      revalidateTag(TAG.adminAllocations, TAG.catalog);
      setConfirming(false);
    } catch (e) {
      setError(errorMessage(e, t));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="space-y-2">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0 space-y-1">
          <p className="text-xs font-semibold uppercase tracking-wide text-ink-soft">{t("admin.alloc.backing.title")}</p>
          <p className="text-xs text-ink-soft">{t(backingHintKey(backing))}</p>
        </div>
        <BackingBadge backing={backing} verbose className="shrink-0" />
      </div>
      {confirming ? (
        <div className="space-y-2 rounded-lg border border-border bg-main-surface p-3">
          <p className="text-xs">{t(next === "cash" ? "admin.alloc.backing.confirmCash" : "admin.alloc.backing.confirmInKind", { service: allocation.service })}</p>
          <div className="flex gap-2">
            <Button type="button" variant="outline" size="sm" className="flex-1" disabled={busy} onClick={() => setConfirming(false)}>
              {t("ui.cancel")}
            </Button>
            <Button type="button" size="sm" className="flex-1" disabled={busy} onClick={flip}>
              {busy ? <Loader2 className="size-3.5 animate-spin" /> : null}
              {t(next === "cash" ? "admin.alloc.backing.markCash" : "admin.alloc.backing.markInKind")}
            </Button>
          </div>
        </div>
      ) : (
        <Button type="button" variant="outline" size="sm" className="w-full" onClick={() => setConfirming(true)}>
          <RefreshCw className="size-3.5" />
          {t(next === "cash" ? "admin.alloc.backing.markCash" : "admin.alloc.backing.markInKind")}
        </Button>
      )}
      {error && (
        <p className="flex items-center gap-2 text-xs text-destructive">
          <TriangleAlert className="size-3.5" /> {error}
        </p>
      )}
    </div>
  );
}
