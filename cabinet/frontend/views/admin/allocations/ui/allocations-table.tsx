"use client";

import { Boxes } from "lucide-react";

import { useT } from "@evinvest/i18n/react";
import { Card, CardContent, Empty, EmptyHeader, EmptyMedia, EmptyTitle, Skeleton, Table, TableBody, TableHead, TableHeader, TableRow } from "@evinvest/uikit";

import type { AllocationWrite } from "@/entities/admin/api/admin-client";
import type { Allocation, AllocationAccessLevel } from "@/shared/contracts/admin";
import { cn } from "@/shared/lib/cn";
import { useIsCompact } from "@/shared/lib/use-is-compact";
import { Settled } from "@/shared/ui/motion";
import type { AllocationPanelKind } from "@/views/admin/allocations/lib/panel";
import { EDGE_CELL, TABLE_HEAD } from "@/views/admin/lib/table";
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
            <div className="p-8">
              <Empty className="border md:p-6">
                <EmptyHeader>
                  <EmptyMedia variant="icon">
                    <Boxes />
                  </EmptyMedia>
                  <EmptyTitle>{t("admin.alloc.empty")}</EmptyTitle>
                </EmptyHeader>
              </Empty>
            </div>
          ) : compact ? (
            <div className="divide-y divide-border">
              {rows.map((row) => (
                <AllocationCard key={row.service} {...propsFor(row)} />
              ))}
            </div>
          ) : (
            // The kit's wrapper scrolls sideways rather than the card clipping: with the side
            // panel open the table can be narrower than its six columns want, and a cut
            // Actions cell must stay reachable, with its half-visible button as the cue.
            <Table>
              <TableHeader>
                {/* i18n-max: 14 per header — auto-layout table; a long header widens its
                    column and squeezes the Product cell. */}
                <TableRow>
                  <TableHead className={cn(TABLE_HEAD, EDGE_CELL)}>{t("admin.alloc.col.product")}</TableHead>
                  <TableHead className={cn(TABLE_HEAD, EDGE_CELL)}>{t("admin.alloc.col.serviceId")}</TableHead>
                  <TableHead className={cn(TABLE_HEAD, EDGE_CELL)}>{t("admin.col.state")}</TableHead>
                  <TableHead className={cn(TABLE_HEAD, EDGE_CELL)}>{t("admin.alloc.col.access")}</TableHead>
                  <TableHead className={cn(TABLE_HEAD, EDGE_CELL)}>{t("admin.alloc.col.unitCap")}</TableHead>
                  <TableHead className={cn(TABLE_HEAD, EDGE_CELL, "text-right")}>{t("admin.col.actions")}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {rows.map((row) => (
                  <AllocationRow key={row.service} {...propsFor(row)} />
                ))}
              </TableBody>
            </Table>
          )}
        </Settled>
      </CardContent>
    </Card>
  );
}
