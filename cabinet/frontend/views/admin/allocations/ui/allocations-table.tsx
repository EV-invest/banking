"use client";

import { useT } from "@evinvest/i18n/react";
import { Card, CardContent, Skeleton } from "@evinvest/uikit";

import type { AllocationWrite } from "@/entities/admin/api/admin-client";
import type { Allocation, AllocationAccessLevel } from "@/shared/contracts/admin";
import { Settled } from "@/shared/ui/motion";
import { AllocationRow } from "@/views/admin/allocations/ui/allocation-row";

export function AllocationsTable({
  rows,
  busyService,
  editingService,
  onEdit,
  onSave,
  onToggle,
  onSetAccess,
  onOpenGrants,
}: {
  rows: Allocation[] | null;
  busyService: string | null;
  editingService: string | null;
  onEdit: (service: string) => void;
  onSave: (body: AllocationWrite) => Promise<void>;
  onToggle: (row: Allocation) => void;
  onSetAccess: (row: Allocation, level: AllocationAccessLevel) => void;
  onOpenGrants: (row: Allocation) => void;
}) {
  const t = useT();
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
          ) : (
            <table className="w-full text-sm">
              <thead>
                {/* i18n-max: 14 per header — auto-layout table with no scroll wrapper; a
                    long header widens its column and squeezes the Product cell. */}
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
                  <AllocationRow
                    // Keyed on the edit flag too, so opening the editor remounts the row and
                    // reseeds its fields from the row as it stands now — see the original
                    // note in the pre-split view for the cancel/reopen hazard this avoids.
                    key={`${row.service}:${editingService === row.service}`}
                    row={row}
                    busy={busyService === row.service}
                    editing={editingService === row.service}
                    onEdit={() => onEdit(row.service)}
                    onSave={onSave}
                    onToggle={() => onToggle(row)}
                    onSetAccess={(level) => onSetAccess(row, level)}
                    onOpenGrants={() => onOpenGrants(row)}
                  />
                ))}
              </tbody>
            </table>
          )}
        </Settled>
      </CardContent>
    </Card>
  );
}
