"use client";

import { Loader2, Users } from "lucide-react";

import { useT } from "@evinvest/i18n/react";
import { Button, Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@evinvest/uikit";

import type { AllocationAccessGrant } from "@/shared/contracts/admin";
import { cn } from "@/shared/lib/cn";
import { accessLabel, accessTone } from "@/views/admin/allocations/lib/access";
import { ago } from "@/views/admin/lib/format";

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
    <table className="w-full text-sm">
      <thead>
        {/* i18n-max: 10 per header — the grants table sits in a 340px panel. */}
        <tr className="border-b border-border text-left text-xs uppercase tracking-wide text-muted-foreground">
          <th className="py-2 font-medium">{t("admin.col.user")}</th>
          <th className="py-2 font-medium">{t("admin.alloc.grants.col.level")}</th>
          <th className="py-2 font-medium">{t("admin.alloc.grants.col.grantedBy")}</th>
          <th className="py-2 text-right font-medium">{t("admin.col.when")}</th>
        </tr>
      </thead>
      <tbody className="divide-y divide-border">
        {grants.map((g) => (
          <tr key={g.user_id}>
            <td className="py-2 pr-2 font-mono-tech text-xs">{g.user_id}</td>
            <td className="py-2 pr-2">
              <span className={cn("inline-flex items-center whitespace-nowrap rounded-full border px-2 py-0.5 text-xs font-medium", accessTone(g.level))}>{accessLabel(g.level, t)}</span>
            </td>
            <td className="py-2 pr-2 font-mono-tech text-xs text-muted-foreground">{g.granted_by}</td>
            <td className="py-2 text-right">
              <div className="flex items-center justify-end gap-2">
                <span className="text-xs text-muted-foreground">{ago(g.granted_at, t)}</span>
                <Button type="button" variant="outline" size="sm" disabled={busyUserId === g.user_id} onClick={() => onRevoke(g.user_id)}>
                  {busyUserId === g.user_id ? <Loader2 className="size-3.5 animate-spin" /> : t("admin.alloc.grants.revoke")}
                </Button>
              </div>
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
