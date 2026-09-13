"use client";

// The per-product issuance panel: who holds the supply today, and the form to mint more
// of it in kind. The same `Panel`-beside-the-table idiom as `GrantsPanel`, for the same
// reason — the operator keeps the row in view while they work it.
//
// No toast: the cabinet mounts no `Toaster`, and a result that names a queued money
// movement should stay on screen beside the split it will change, not slide away.

import { CheckCircle2, Clock, TriangleAlert, X } from "lucide-react";
import { useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Card, CardContent, Skeleton } from "@evinvest/uikit";

import { issueUnits, type IssueUnitsBody } from "@/entities/admin/api/admin-client";
import { unitHoldersResource } from "@/entities/admin/model/admin-resource";
import type { Allocation, UnitIssuance } from "@/shared/contracts/admin";
import { errorMessage } from "@/shared/lib/api-client";
import { TAG } from "@/shared/lib/cache-tags";
import { formatUnits } from "@/shared/lib/money";
import { revalidateTag, useResource } from "@/shared/lib/resource";
import { Settled } from "@/shared/ui/motion";
import { HoldersTable } from "@/views/admin/allocations/ui/holders-table";
import { IssueForm } from "@/views/admin/allocations/ui/issue-form";

interface Outcome {
  issuance: UnitIssuance;
  holderLabel: string;
}

export function IssuancePanel({ allocation, onClose }: { allocation: Allocation; onClose: () => void }) {
  const t = useT();
  const [busy, setBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [last, setLast] = useState<Outcome | null>(null);

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
    // Fixed width, matching `GrantsPanel` — see the note there.
    <Card className="w-85">
      <CardContent className="space-y-5 py-5">
        <div className="flex items-start justify-between gap-2">
          <div className="min-w-0">
            <p className="truncate font-semibold">{allocation.title}</p>
            <p className="truncate font-mono-tech text-xs text-muted-foreground">{allocation.service}</p>
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label={t("ui.close")}
            className="rounded-md text-muted-foreground outline-none transition-colors hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
          >
            <X className="size-4" />
          </button>
        </div>

        {error && (
          <p className="flex items-center gap-2 text-xs text-destructive">
            <TriangleAlert className="size-3.5" /> {error}
          </p>
        )}

        <div className="space-y-2">
          <p className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">{t("admin.alloc.issue.title")}</p>
          <IssueForm service={allocation.service} busy={busy} onSubmit={issue} />
          {last && <Result outcome={last} />}
          <p className="text-xs text-muted-foreground">{t("admin.alloc.issue.note")}</p>
        </div>

        <div className="space-y-2">
          <p className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">{t("admin.alloc.holders.title")}</p>
          <Settled loading={!read.data} skeleton={<Skeleton className="h-24 w-full" />}>
            {read.data && <HoldersTable holders={read.data} />}
          </Settled>
        </div>
      </CardContent>
    </Card>
  );
}

/** The last mint, as the hub answered it. `queued` is the ordinary answer — the relay
 *  posts the mint after the POST returns — so it is shown as a state, not as a warning. */
function Result({ outcome }: { outcome: Outcome }) {
  const t = useT();
  const applied = outcome.issuance.state === "applied";
  const args = { units: formatUnits(outcome.issuance.units), holder: outcome.holderLabel };
  return (
    <p className="flex items-start gap-2 text-xs text-main-accent-t2">
      {applied ? <CheckCircle2 className="mt-0.5 size-3.5 shrink-0" /> : <Clock className="mt-0.5 size-3.5 shrink-0" />}
      <span>{t(applied ? "admin.alloc.issue.resultApplied" : "admin.alloc.issue.resultQueued", args)}</span>
    </p>
  );
}
