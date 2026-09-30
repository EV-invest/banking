"use client";

import { ShieldCheck, UserMinus } from "lucide-react";

import { useLocale, useT } from "@evinvest/i18n/react";
import { Badge, Button, Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle, Spinner, Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@evinvest/uikit";

import type { ScopeHolder } from "@/shared/contracts";
import { holderName } from "@/features/panel-access/lib/holder";
import { formatDay } from "@/shared/lib/datetime";

const HEAD = "text-xs font-medium text-ink-soft";

export function ScopeHoldersTable({
  holders,
  busyUserId,
  canRevoke,
  onRevoke,
}: {
  holders: readonly ScopeHolder[];
  busyUserId: string | null;
  canRevoke: (holderRole: string | undefined) => boolean;
  onRevoke: (holder: ScopeHolder) => void;
}) {
  const t = useT();
  const locale = useLocale();

  if (holders.length === 0) {
    return (
      <Empty className="border">
        <EmptyHeader>
          <EmptyMedia variant="icon">
            <ShieldCheck />
          </EmptyMedia>
          <EmptyTitle>{t("panelAccess.empty")}</EmptyTitle>
          <EmptyDescription>{t("panelAccess.emptyHint")}</EmptyDescription>
        </EmptyHeader>
      </Empty>
    );
  }

  return (
    <Table>
      <TableHeader>
        {/* i18n-max: 10 per header — the roster sits in a 340px panel. */}
        <TableRow>
          <TableHead className={HEAD}>{t("panelAccess.col.person")}</TableHead>
          <TableHead className={HEAD}>{t("panelAccess.col.role")}</TableHead>
          <TableHead className="sr-only">{t("panelAccess.revoke")}</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {holders.map((h) => {
          const userId = h.grant?.user_id ?? "";
          const name = holderName(h);
          return (
            <TableRow key={userId}>
              {/* Three columns, not four: in the 340px side panel a separate date column
                  pushed the revoke control out of sight, so the grant date rides under the
                  person. Who granted it is not shown: the plane hands back a bare user id,
                  and an id is not something a reader can use. */}
              <TableCell className="max-w-40">
                <div className="truncate text-sm" title={h.email}>
                  {h.email || "—"}
                </div>
                {name && <div className="truncate text-xs text-ink-soft">{name}</div>}
                <div className="text-xs text-ink-soft tabular-nums">{formatDay(h.grant?.granted_at === undefined ? null : String(h.grant.granted_at), locale)}</div>
              </TableCell>
              <TableCell>
                <Badge variant={h.grant?.role === "admin" ? "secondary" : "outline"}>{t(`admin.role.${h.grant?.role === "admin" ? "admin" : "operator"}`)}</Badge>
              </TableCell>
              <TableCell className="text-right">
                {canRevoke(h.grant?.role) && (
                  // An icon, not the word: the panel is 340px and the word is spelled out in
                  // the confirmation this opens.
                  <Button type="button" icon variant="ghost" size="sm" aria-label={t("panelAccess.revoke")} title={t("panelAccess.revoke")} disabled={busyUserId !== null} onClick={() => onRevoke(h)}>
                    {busyUserId === userId ? <Spinner /> : <UserMinus className="size-4" aria-hidden />}
                  </Button>
                )}
              </TableCell>
            </TableRow>
          );
        })}
      </TableBody>
    </Table>
  );
}
