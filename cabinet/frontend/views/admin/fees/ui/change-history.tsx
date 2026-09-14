"use client";

// Every version of one product's terms, newest first — what was scheduled, by whom, when
// it bound, and why. Nothing is deleted: a cancelled or rejected change stays readable,
// because the terms an investor was on, and how they came to be, are public facts about
// the product (fees.proto, `ListFeePolicyChanges`).

import { History } from "lucide-react";

import { useLocale, useT } from "@evinvest/i18n/react";
import { Badge, Card, CardContent, Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle, Skeleton } from "@evinvest/uikit";

import { feePolicyChangesResource } from "@/entities/admin/model/admin-resource";
import { cn } from "@/shared/lib/cn";
import { formatMoment, hasStamp } from "@/shared/lib/datetime";
import { useResource } from "@/shared/lib/resource";
import { ResourceError } from "@/shared/ui/resource-error";
import { changeStateLabel, changeStateTone, termsSummary } from "@/views/admin/fees/lib/format";

export function ChangeHistory({ service }: { service: string }) {
  const t = useT();
  const locale = useLocale();
  const list = useResource(feePolicyChangesResource, service);
  const rows = list.data?.changes ?? [];

  return (
    <Card>
      <CardContent className="space-y-4 py-6">
        <p className="text-sm font-semibold">{t("admin.fees.history")}</p>
        {list.isLoading ? (
          <Skeleton className="h-24 w-full" />
        ) : !list.data && list.error ? (
          // In place of the zero state, never as it: "no changes yet" is a claim about the
          // fund's history that a read which did not arrive has not earned.
          <ResourceError error={list.error} onRetry={() => void list.refresh()} retrying={list.isValidating} />
        ) : rows.length === 0 ? (
          <Empty className="border">
            <EmptyHeader>
              <EmptyMedia variant="icon">
                <History />
              </EmptyMedia>
              <EmptyTitle>{t("admin.fees.noHistory")}</EmptyTitle>
              <EmptyDescription>{t("admin.fees.noHistoryHint")}</EmptyDescription>
            </EmptyHeader>
          </Empty>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border text-left text-xs uppercase tracking-wide text-muted-foreground">
                  <th className="pb-2 font-medium">{t("admin.fees.col.version")}</th>
                  <th className="pb-2 font-medium">{t("admin.col.state")}</th>
                  <th className="pb-2 font-medium">{t("admin.fees.terms")}</th>
                  <th className="pb-2 font-medium">{t("admin.fees.col.effective")}</th>
                  <th className="pb-2 font-medium">{t("admin.fees.col.reason")}</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((c) => (
                  <tr key={c.id} className="border-b border-border/50 align-top last:border-0">
                    <td className="py-2 tabular-nums">{c.version}</td>
                    <td className="py-2">
                      <Badge variant="outline" className={cn(changeStateTone(c.state))}>
                        {changeStateLabel(c.state, t)}
                      </Badge>
                    </td>
                    <td className="py-2 tabular-nums">{termsSummary(c, t)}</td>
                    {/* "0" while a change awaits the owners: the moment is not known until
                        they carry it, and a dash says so better than 1 Jan 1970 would. */}
                    <td className="py-2 tabular-nums text-muted-foreground">{hasStamp(c.effective_from) ? formatMoment(c.effective_from, locale) : "—"}</td>
                    <td className="max-w-xs whitespace-pre-line py-2 text-muted-foreground">{c.reason.trim() || "—"}</td>
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
