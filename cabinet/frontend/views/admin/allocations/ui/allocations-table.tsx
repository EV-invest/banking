"use client";

import { useT } from "@evinvest/i18n/react";
import { Card, CardContent, Skeleton } from "@evinvest/uikit";

import type { AllocationWrite } from "@/entities/admin/api/admin-client";
import type { Allocation, AllocationAccessLevel } from "@/shared/contracts/admin";
import { useIsCompact } from "@/shared/lib/use-is-compact";
import { Settled } from "@/shared/ui/motion";
import type { AllocationPanelKind } from "@/views/admin/allocations/lib/panel";
import { AllocationCard } from "@/views/admin/allocations/ui/allocation-card";
import { AllocationRow, type AllocationRowProps } from "@/views/admin/allocations/ui/allocation-row";

export function AllocationsTable({
  rows,
  busyService,
  editingService,
  onEdit,
  onSave,
  onToggle,
  onSetAccess,
  onOpenPanel,
}: {
  rows: Allocation[] | null;
  busyService: string | null;
  editingService: string | null;
  onEdit: (service: string) => void;
  onSave: (body: AllocationWrite) => Promise<void>;
  onToggle: (row: Allocation) => void;
  onSetAccess: (row: Allocation, level: AllocationAccessLevel) => void;
  onOpenPanel: (row: Allocation, kind: AllocationPanelKind) => void;
}) {
  const t = useT();
  // Below `md` the rows are cards — see `AllocationCard` for why a sideways-scrolling
  // table is the wrong answer on a phone.
  const compact = useIsCompact("md");

  const propsFor = (row: Allocation): AllocationRowProps => ({
    row,
    busy: busyService === row.service,
    editing: editingService === row.service,
    onEdit: () => onEdit(row.service),
    onSave,
    onToggle: () => onToggle(row),
    onSetAccess: (level) => onSetAccess(row, level),
    onOpenPanel: (kind) => onOpenPanel(row, kind),
  });

  return (
    <Card>
      <CardContent className="p-0">
        <Settled
          loading={!rows}
          skeleton={
            <div className="p-6">
              <Skeleton className="h-32 w-full" />
            </div>
          }
        >
          {!rows ? null : rows.length === 0 ? (
            <p className="p-8 text-center text-sm text-muted-foreground">{t("admin.alloc.empty")}</p>
          ) : compact ? (
            <div className="divide-y divide-border">
              {rows.map((row) => (
                <AllocationCard key={row.service} {...propsFor(row)} />
              ))}
            </div>
          ) : (
            // The wrapper scrolls sideways rather than the card clipping: with the side
            // panel open the table can be narrower than its six columns want, and a cut
            // Actions cell must stay reachable, with its half-visible button as the cue.
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  {/* i18n-max: 14 per header — auto-layout table; a long header widens its
                      column and squeezes the Product cell. */}
                  <tr className="border-b border-border text-left text-xs uppercase tracking-wide text-muted-foreground">
                    <th className="px-5 py-3 font-medium">{t("admin.alloc.col.product")}</th>
                    <th className="px-5 py-3 font-medium">{t("admin.alloc.col.serviceId")}</th>
                    <th className="px-5 py-3 font-medium">{t("admin.col.state")}</th>
                    <th className="px-5 py-3 font-medium">{t("admin.alloc.col.access")}</th>
                    <th className="px-5 py-3 font-medium">{t("admin.alloc.col.unitCap")}</th>
                    <th className="px-5 py-3 text-right font-medium">{t("admin.col.actions")}</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-border">
                  {rows.map((row) => (
                    <AllocationRow key={row.service} {...propsFor(row)} />
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </Settled>
      </CardContent>
    </Card>
  );
}
