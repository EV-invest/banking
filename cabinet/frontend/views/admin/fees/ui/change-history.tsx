"use client";

// Every version of one product's terms, newest first — what was scheduled, by whom, when
// it bound, and why. Nothing is deleted: a cancelled or rejected change stays readable,
// because the terms an investor was on, and how they came to be, are public facts about
// the product (fees.proto, `ListFeePolicyChanges`).

import { History } from "lucide-react";

import { useLocale, useT } from "@evinvest/i18n/react";
import { Badge, Card, CardContent, Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle, Skeleton, Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@evinvest/uikit";

import { feePolicyChangesResource } from "@/entities/admin/model/admin-resource";
import { cn } from "@/shared/lib/cn";
import { formatMoment, hasStamp } from "@/shared/lib/datetime";
import { useResource } from "@/shared/lib/resource";
import { ResourceError } from "@/shared/ui/resource-error";
import { changeStateLabel, changeStateTone, termsSummary } from "@/views/admin/fees/lib/format";
import { WaiverNote } from "@/views/admin/fees/ui/notice-waiver";

// The house table idiom (`views/trade/ui/fills-table.tsx`): uikit's `Table` carries the
// borders, the cell padding and the scroll wrapper; only the header treatment is ours.
const HEAD = "h-8 text-xs font-medium uppercase tracking-wide text-muted-foreground";

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
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className={HEAD}>{t("admin.fees.col.version")}</TableHead>
                <TableHead className={HEAD}>{t("admin.col.state")}</TableHead>
                <TableHead className={HEAD}>{t("admin.fees.terms")}</TableHead>
                <TableHead className={HEAD}>{t("admin.fees.col.effective")}</TableHead>
                <TableHead className={HEAD}>{t("admin.fees.col.reason")}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {rows.map((c) => (
                <TableRow key={c.id}>
                  <TableCell className="align-top tabular-nums">{c.version}</TableCell>
                  <TableCell className="align-top">
                    <Badge variant="outline" className={cn(changeStateTone(c.state))}>
                      {changeStateLabel(c.state, t)}
                    </Badge>
                    <WaiverNote change={c} />
                  </TableCell>
                  <TableCell className="align-top tabular-nums">{termsSummary(c, t)}</TableCell>
                  {/* "0" while a change awaits the owners: the moment is not known until
                      they carry it, and a dash says so better than 1 Jan 1970 would. */}
                  <TableCell className="align-top tabular-nums text-muted-foreground">{hasStamp(c.effective_from) ? formatMoment(c.effective_from, locale) : "—"}</TableCell>
                  {/* The one free-text column: it wraps (uikit cells default to nowrap) so a
                      long reason costs height, not a sideways scroll of the whole table. */}
                  <TableCell className="max-w-xs align-top whitespace-normal text-muted-foreground">{c.reason.trim() || "—"}</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        )}
      </CardContent>
    </Card>
  );
}
