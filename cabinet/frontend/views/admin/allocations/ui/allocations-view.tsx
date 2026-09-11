"use client";

import { Plus } from "lucide-react";
import { useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Button } from "@evinvest/uikit";

import { registerAllocation, setAllocationAccess, setAllocationState, updateAllocation } from "@/entities/admin/api/admin-client";
import { adminAllocationsResource } from "@/entities/admin/model/admin-resource";
import type { Allocation } from "@/shared/contracts/admin";
import { errorMessage } from "@/shared/lib/api-client";
import { TAG } from "@/shared/lib/cache-tags";
import { cn } from "@/shared/lib/cn";
import { revalidateTag, useResource } from "@/shared/lib/resource";
import { Panel, PanelPresence, PanelSwap, StaggerItem } from "@/shared/ui/motion";
import { ResourceError } from "@/shared/ui/resource-error";
import { AllocationsTable } from "@/views/admin/allocations/ui/allocations-table";
import { GrantsPanel } from "@/views/admin/allocations/ui/grants-panel";
import { RegisterForm } from "@/views/admin/allocations/ui/register-form";
import { AdminHeader, AdminScreen } from "@/views/admin/ui/shell";

const TEAL_CTA = "bg-main-accent-t1 text-main-black hover:bg-main-accent-t1/90";

export function AllocationsView() {
  const t = useT();
  const [actionError, setActionError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [editing, setEditing] = useState<string | null>(null);
  const [grantsFor, setGrantsFor] = useState<Allocation | null>(null);

  const read = useResource(adminAllocationsResource);
  const rows = read.data ? (read.data.allocations ?? []) : null;
  const error = actionError ?? (read.data || !read.error ? null : errorMessage(read.error, t));

  const run = async (key: string, fn: () => Promise<unknown>) => {
    setBusy(key);
    setActionError(null);
    try {
      await fn();
      // Registering, renaming, opening or re-gating a fund also changes what investors
      // see: the rail's Products group, the fund picker, the invest card's badge and the
      // subscribe control's own gate all read the investor-facing catalog.
      revalidateTag(TAG.catalog);
      await read.refresh();
      return true;
    } catch (e) {
      setActionError(errorMessage(e, t));
      return false;
    } finally {
      setBusy(null);
    }
  };

  return (
    <AdminScreen className="space-y-8">
      <AdminHeader
        eyebrow={t("admin.eyebrow.administer")}
        title={t("nav.allocations")}
        subtitle={t("admin.alloc.subtitle")}
        action={
          <Button type="button" className={cn(TEAL_CTA)} onClick={() => setAdding((v) => !v)}>
            <Plus className="size-4" />
            {t("admin.alloc.register")}
          </Button>
        }
      />

      {error && <ResourceError message={error} />}

      {adding && <RegisterForm busy={busy === "register"} onCancel={() => setAdding(false)} onSubmit={async (body) => (await run("register", () => registerAllocation(body))) && setAdding(false)} />}

      <StaggerItem as="section" className="flex gap-6">
        <div className="min-w-0 flex-1 space-y-3">
          <p className="flex items-center gap-2 text-xs font-semibold uppercase tracking-widest text-muted-foreground">
            {t("admin.alloc.registry")}
            {rows && <span className="rounded-full bg-main-accent-t1/15 px-2 py-0.5 text-xs font-semibold text-main-accent-t1">{rows.length}</span>}
          </p>
          <AllocationsTable
            rows={rows}
            busyService={busy}
            editingService={editing}
            onEdit={(service) => setEditing((current) => (current === service ? null : service))}
            onSave={async (body) => {
              if (await run(body.service, () => updateAllocation(body))) setEditing(null);
            }}
            onToggle={(row) => run(row.service, () => setAllocationState(row.service, row.state === "open" ? "closed" : "open"))}
            onSetAccess={(row, level) => run(row.service, () => setAllocationAccess(row.service, level))}
            onOpenGrants={(row) => setGrantsFor((current) => (current?.service === row.service ? null : row))}
          />
          {/* One key for the whole paragraph, with the state name interpolated: a translator
              has to be able to move `draft` to wherever the sentence puts it in their
              language, which splitting the note around the `<span>` would forbid. */}
          <p className="max-w-3xl text-xs text-muted-foreground">{t("admin.alloc.footnote", { state: t("admin.state.draft") })}</p>
        </div>

        {/* Same collapse-width panel idiom as the Users screen's row drawer — see
            `UserDrawer` in `views/admin/users/ui/users-view.tsx` for why the width is
            fixed rather than `w-full`, and why the presence boundary lives up here. */}
        <PanelPresence>
          {grantsFor && (
            <Panel key="grants-panel" collapse={{ gap: "1.5rem", width: "21.25rem" }} className="shrink-0 self-start overflow-hidden">
              <PanelSwap swapKey={grantsFor.service}>
                <GrantsPanel key={grantsFor.service} allocation={grantsFor} onClose={() => setGrantsFor(null)} />
              </PanelSwap>
            </Panel>
          )}
        </PanelPresence>
      </StaggerItem>
    </AdminScreen>
  );
}
