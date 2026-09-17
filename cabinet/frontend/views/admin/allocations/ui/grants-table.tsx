"use client";

import { Users } from "lucide-react";

import { useT } from "@evinvest/i18n/react";
import { Button, Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle, Spinner, Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@evinvest/uikit";

import type { AllocationAccessGrant } from "@/shared/contracts/admin";
import { cn } from "@/shared/lib/cn";
import { accessLabel, accessTone } from "@/views/admin/allocations/lib/access";
import { ago } from "@/views/admin/lib/format";
import { TABLE_HEAD } from "@/views/admin/lib/table";

export function GrantsTable({ grants, busyUserId, onRevoke }: { grants: AllocationAccessGrant[]; busyUserId: string | null; onRevoke: (userId: string) => void }) {
  const t = useT();

  if (grants.length === 0) {
    return (
      <Empty className="border">
        <EmptyHeader>
          <EmptyMedia variant="icon">
            <Users />
          </EmptyMedia>
          <EmptyTitle>{t("admin.alloc.grants.empty")}</EmptyTitle>
          <EmptyDescription>{t("admin.alloc.grants.emptyHint")}</EmptyDescription>
        </EmptyHeader>
      </Empty>
    );
  }

  return (
    <Table>
      <TableHeader>
        {/* i18n-max: 10 per header — the grants table sits in a 340px panel. */}
        <TableRow>
          <TableHead className={TABLE_HEAD}>{t("admin.col.user")}</TableHead>
          <TableHead className={TABLE_HEAD}>{t("admin.alloc.grants.col.level")}</TableHead>
          <TableHead className={TABLE_HEAD}>{t("admin.alloc.grants.col.grantedBy")}</TableHead>
          <TableHead className={cn(TABLE_HEAD, "text-right")}>{t("admin.col.when")}</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {grants.map((g) => (
          <TableRow key={g.user_id}>
            {/* Capped width so a long email truncates instead of pushing the other columns
                out of the 340px panel — `truncate` alone does nothing in an auto-layout cell. */}
            <TableCell className="max-w-32" title={g.user_id}>
              {g.email ? (
                <>
                  <div className="truncate text-sm" title={g.email}>
                    {g.email}
                  </div>
                  <div className="truncate font-mono-tech text-xs text-ink-soft">{g.user_id}</div>
                </>
              ) : (
                <div className="truncate font-mono-tech text-xs">{g.user_id}</div>
              )}
            </TableCell>
            <TableCell>
              <span className={cn("inline-flex items-center whitespace-nowrap rounded-full border px-2 py-0.5 text-xs font-medium", accessTone(g.level))}>{accessLabel(g.level, t)}</span>
            </TableCell>
            <TableCell className="font-mono-tech text-xs text-ink-soft">{g.granted_by}</TableCell>
            <TableCell className="text-right">
              <div className="flex items-center justify-end gap-2">
                <span className="text-xs text-ink-soft">{ago(g.granted_at, t)}</span>
                <Button type="button" variant="outline" size="sm" disabled={busyUserId === g.user_id} onClick={() => onRevoke(g.user_id)}>
                  {busyUserId === g.user_id ? <Spinner className="size-3.5" aria-hidden /> : t("admin.alloc.grants.revoke")}
                </Button>
              </div>
            </TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  );
}
