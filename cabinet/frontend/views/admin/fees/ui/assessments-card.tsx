"use client";

// The audit trail: every charge this fund has made, newest first.

import { Percent } from "lucide-react";

import { useT } from "@evinvest/i18n/react";
import { Card, CardContent, Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle, Skeleton, Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@evinvest/uikit";

import { feeAssessmentsResource } from "@/entities/admin/model/admin-resource";
import { cn } from "@/shared/lib/cn";
import { useResource } from "@/shared/lib/resource";
import { ResourceError } from "@/shared/ui/resource-error";
import { triggerLabel } from "@/views/admin/fees/lib/format";
import { ago, formatUnits, formatUsdt } from "@/views/admin/lib/format";

// The house table idiom (`views/trade/ui/fills-table.tsx`): uikit's `Table` carries the
// borders, the cell padding and the scroll wrapper; only the header treatment is ours.
const HEAD = "h-8 text-xs font-medium uppercase tracking-wide text-muted-foreground";

export function AssessmentsCard({ service }: { service: string }) {
  const t = useT();
  const list = useResource(feeAssessmentsResource, service);
  const rows = list.data?.assessments ?? [];

  return (
    <Card>
      <CardContent className="space-y-4 py-6">
        <p className="text-sm font-semibold">{t("admin.fees.charges")}</p>
        {list.isLoading ? (
          <Skeleton className="h-24 w-full" />
        ) : !list.data && list.error ? (
          // In place of the zero state, never as it: "nobody has been billed" is a claim a
          // read that did not arrive has not earned.
          <ResourceError error={list.error} onRetry={() => void list.refresh()} retrying={list.isValidating} />
        ) : rows.length === 0 ? (
          <Empty className="border">
            <EmptyHeader>
              <EmptyMedia variant="icon">
                <Percent />
              </EmptyMedia>
              <EmptyTitle>{t("admin.fees.noCharges")}</EmptyTitle>
              <EmptyDescription>{t("admin.fees.noChargesHint")}</EmptyDescription>
            </EmptyHeader>
          </Empty>
        ) : (
          <Table>
            <TableHeader>
              {/* i18n-max: 14 per header — the wrapper scrolls, so a long header costs a
                  sideways drag rather than a clipped column. */}
              <TableRow>
                <TableHead className={HEAD}>{t("admin.col.when")}</TableHead>
                <TableHead className={HEAD}>{t("admin.fees.col.trigger")}</TableHead>
                <TableHead className={cn(HEAD, "text-right")}>{t("admin.fees.field.management")}</TableHead>
                <TableHead className={cn(HEAD, "text-right")}>{t("admin.fees.field.performance")}</TableHead>
                <TableHead className={cn(HEAD, "text-right")}>{t("admin.fees.col.unitsTaken")}</TableHead>
                <TableHead className={cn(HEAD, "text-right")}>{t("admin.fees.col.deferred")}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {rows.map((a, i) => (
                <TableRow key={`${a.assessed_at}-${i}`}>
                  <TableCell className="text-muted-foreground">{ago(a.assessed_at, t)}</TableCell>
                  <TableCell>{triggerLabel(a.trigger, t)}</TableCell>
                  <TableCell className="text-right tabular-nums">{formatUsdt(a.management)}</TableCell>
                  <TableCell className="text-right tabular-nums">{formatUsdt(a.performance)}</TableCell>
                  <TableCell className="text-right tabular-nums">{formatUnits(a.charged_units)}</TableCell>
                  {/* Non-zero means the holding could not cover the charge and the rest
                      rides to the next one. Worth its own column: it is the only reason
                      a charge collects less than it assessed. */}
                  <TableCell className={cn("text-right tabular-nums", Number(a.debt_carried) > 0 ? "text-main-accent-t3" : "text-muted-foreground")}>
                    {formatUsdt(a.debt_carried)}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </CardContent>
    </Card>
  );
}
