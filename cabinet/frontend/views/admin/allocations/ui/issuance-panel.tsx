"use client";

// The per-product issuance panel: who holds the supply today, and the form to mint more
// of it in kind. The same `Panel`-beside-the-table idiom as `GrantsPanel`, for the same
// reason — the operator keeps the row in view while they work it.
//
// No toast: the cabinet mounts no `Toaster`, and a result that names a queued money
// movement should stay on screen beside the split it will change, not slide away.

import { TriangleAlert } from "lucide-react";
import { useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Card, CardContent, Skeleton } from "@evinvest/uikit";

import { issueUnits, type IssueUnitsBody } from "@/entities/admin/api/admin-client";
import { unitHoldersResource } from "@/entities/admin/model/admin-resource";
import type { Allocation } from "@/shared/contracts/admin";
import { errorMessage } from "@/shared/lib/api-client";
import { TAG } from "@/shared/lib/cache-tags";
import { cn } from "@/shared/lib/cn";
import { revalidateTag, useResource } from "@/shared/lib/resource";
import { Settled } from "@/shared/ui/motion";
import { HoldersTable } from "@/views/admin/allocations/ui/holders-table";
import { IssuanceResult, type IssuanceOutcome } from "@/views/admin/allocations/ui/issuance-result";
import { IssueForm } from "@/views/admin/allocations/ui/issue-form";
import { PanelHeader } from "@/views/admin/allocations/ui/panel-header";
import { PinCapAction } from "@/views/admin/allocations/ui/pin-cap-action";
import { TransferStakeAction } from "@/views/admin/allocations/ui/transfer-stake-action";

export function IssuancePanel({ allocation, onClose, className }: { allocation: Allocation; onClose: () => void; className?: string }) {
  const t = useT();
  const [busy, setBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [last, setLast] = useState<IssuanceOutcome | null>(null);

  const read = useResource(unitHoldersResource, allocation.service);
  const error = actionError ?? (read.data || !read.error ? null : errorMessage(read.error, t));

  const issue = async (body: IssueUnitsBody, holderLabel: string) => {
    setBusy(true);
    setActionError(null);
    try {
      const issuance = await issueUnits(body);
      setLast({ issuance, holderLabel });
      // A mint moves the supply and, through it, the mark's `units_outstanding` and
      // `company_units` — the product page and the fund cards read the latter. Both are
      // named even for a `queued` row: the refresh shows the split as it stands, and the
      // resource's own cadence picks the posted mint up when the relay lands it.
      revalidateTag(TAG.adminUnitHolders, TAG.nav);
      await read.refresh();
      return true;
    } catch (e) {
      setActionError(errorMessage(e, t));
      return false;
    } finally {
      setBusy(false);
    }
  };

  return (
    // Fixed width, matching `GrantsPanel` — see the note there. `className` lets the
    // bottom-sheet presentation widen it to the sheet instead.
    <Card className={cn("w-85", className)}>
      <CardContent className="space-y-5 py-5">
        <PanelHeader allocation={allocation} onClose={onClose} />

        {error && (
          <p className="flex items-center gap-2 text-xs text-destructive">
            <TriangleAlert className="size-3.5" /> {error}
          </p>
        )}

        <div className="space-y-2">
          <p className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">{t("admin.alloc.issue.title")}</p>
          <IssueForm service={allocation.service} busy={busy} onSubmit={issue} />
          {last && <IssuanceResult outcome={last} kind="issue" />}
          <p className="text-xs text-muted-foreground">{t("admin.alloc.issue.note")}</p>
        </div>

        <div className="space-y-3">
          <p className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">{t("admin.alloc.holders.title")}</p>
          <Settled loading={!read.data} skeleton={<Skeleton className="h-24 w-full" />}>
            {read.data && (
              <>
                <HoldersTable holders={read.data} />
                <PinCapAction allocation={allocation} holders={read.data} />
                <TransferStakeAction allocation={allocation} holders={read.data} />
              </>
            )}
          </Settled>
        </div>
      </CardContent>
    </Card>
  );
}
