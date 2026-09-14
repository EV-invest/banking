"use client";

// The audit trail: every charge this fund has made, newest first.

import { Percent } from "lucide-react";

import { useT } from "@evinvest/i18n/react";
import { Card, CardContent, Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle, Skeleton } from "@evinvest/uikit";

import { feeAssessmentsResource } from "@/entities/admin/model/admin-resource";
import { useResource } from "@/shared/lib/resource";
import { ago, formatUnits, formatUsd } from "@/views/admin/lib/format";

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
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                {/* i18n-max: 14 per header — the wrapper scrolls, so a long header costs a
                    sideways drag rather than a clipped column. */}
                <tr className="border-b border-border text-left text-xs uppercase tracking-wide text-muted-foreground">
                  <th className="pb-2 font-medium">{t("admin.col.when")}</th>
                  <th className="pb-2 font-medium">{t("admin.fees.col.trigger")}</th>
                  <th className="pb-2 text-right font-medium">{t("admin.fees.field.management")}</th>
                  <th className="pb-2 text-right font-medium">{t("admin.fees.field.performance")}</th>
                  <th className="pb-2 text-right font-medium">{t("admin.fees.col.unitsTaken")}</th>
                  <th className="pb-2 text-right font-medium">{t("admin.fees.col.deferred")}</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((a, i) => (
                  <tr key={`${a.assessed_at}-${i}`} className="border-b border-border/50 last:border-0">
                    <td className="py-2 text-muted-foreground">{ago(a.assessed_at, t)}</td>
                    <td className="py-2 capitalize">{a.trigger}</td>
                    <td className="py-2 text-right tabular-nums">{formatUsd(a.management)}</td>
                    <td className="py-2 text-right tabular-nums">{formatUsd(a.performance)}</td>
                    <td className="py-2 text-right tabular-nums">{formatUnits(a.charged_units)}</td>
                    {/* Non-zero means the holding could not cover the charge and the rest
                        rides to the next one. Worth its own column: it is the only reason
                        a charge collects less than it assessed. */}
                    <td className={`py-2 text-right tabular-nums ${Number(a.debt_carried) > 0 ? "text-main-accent-t3" : "text-muted-foreground"}`}>
                      {formatUsd(a.debt_carried)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </CardContent>
    </Card>
  );
}
