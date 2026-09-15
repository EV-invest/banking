"use client";

// The per-product access-grant panel: who is raised above the row's default, and the form
// to raise or drop someone. Opened from `AllocationsView` the same way the Users screen
// opens its own row drawer — a `Panel` beside the table rather than a modal over it, so an
// operator can see the row the panel belongs to while they work it.

import { TriangleAlert } from "lucide-react";
import { useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Card, CardContent, Skeleton } from "@evinvest/uikit";

import { grantAllocationAccess, revokeAllocationAccess } from "@/entities/admin/api/admin-client";
import { allocationAccessGrantsResource } from "@/entities/admin/model/admin-resource";
import type { Allocation, AllocationGrantLevel } from "@/shared/contracts/admin";
import { errorMessage } from "@/shared/lib/api-client";
import { cn } from "@/shared/lib/cn";
import { revalidateTag, useResource } from "@/shared/lib/resource";
import { TAG } from "@/shared/lib/cache-tags";
import { Settled } from "@/shared/ui/motion";
import { GrantForm } from "@/views/admin/allocations/ui/grant-form";
import { GrantsTable } from "@/views/admin/allocations/ui/grants-table";
import { PanelHeader } from "@/views/admin/allocations/ui/panel-header";

export function GrantsPanel({ allocation, onClose, className }: { allocation: Allocation; onClose: () => void; className?: string }) {
  const t = useT();
  const [busy, setBusy] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);

  const read = useResource(allocationAccessGrantsResource, allocation.service);
  const grants = read.data?.grants ?? [];
  const error = actionError ?? (read.data || !read.error ? null : errorMessage(read.error, t));

  const run = async (key: string, fn: () => Promise<unknown>) => {
    setBusy(key);
    setActionError(null);
    try {
      await fn();
      revalidateTag(TAG.adminAllocationGrants);
      await read.refresh();
    } catch (e) {
      setActionError(errorMessage(e, t));
    } finally {
      setBusy(null);
    }
  };

  const grant = (userId: string, level: AllocationGrantLevel) => run(`grant:${userId}`, () => grantAllocationAccess(allocation.service, userId, level));
  const revoke = (userId: string) => run(`revoke:${userId}`, () => revokeAllocationAccess(allocation.service, userId));

  return (
    // Fixed width, matching `UserDrawer` — the panel is computed once and clipped by the
    // animating wrapper, never itself laid out against the wrapper's own animating width.
    // `className` lets the bottom-sheet presentation widen it to the sheet instead.
    <Card className={cn("w-85", className)}>
      <CardContent className="space-y-5 py-5">
        <PanelHeader allocation={allocation} onClose={onClose} />

        {error && (
          <p className="flex items-center gap-2 text-xs text-destructive">
            <TriangleAlert className="size-3.5" /> {error}
          </p>
        )}

        <div className="space-y-2">
          <p className="text-xs font-semibold uppercase tracking-wide text-ink-soft">{t("admin.alloc.grants.raise")}</p>
          <GrantForm busy={busy?.startsWith("grant:") ?? false} onSubmit={grant} />
        </div>

        <div className="space-y-2">
          <p className="text-xs font-semibold uppercase tracking-wide text-ink-soft">{t("admin.alloc.grants.roster")}</p>
          <Settled loading={!read.data} skeleton={<Skeleton className="h-24 w-full" />}>
            <GrantsTable grants={grants} busyUserId={busy?.startsWith("revoke:") ? busy.slice("revoke:".length) : null} onRevoke={revoke} />
          </Settled>
        </div>
      </CardContent>
    </Card>
  );
}
