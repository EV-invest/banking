"use client";

import { ShieldCheck, UserMinus } from "lucide-react";

import { useLocale, useT } from "@evinvest/i18n/react";
import { Badge, Button, Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle, Spinner, Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@evinvest/uikit";

import type { GrantHolder } from "@/shared/contracts";
import { holderName } from "@/features/panel-access/lib/holder";
import { formatDay } from "@/shared/lib/datetime";

const HEAD = "text-xs font-medium text-ink-soft";

// One row per grant, not per person: a person can hold several targets in the namespace,
// and each is revoked on its own.
export function GrantHoldersTable({ holders, busyGrantId, onRevoke }: { holders: readonly GrantHolder[]; busyGrantId: string | null; onRevoke: (holder: GrantHolder) => void }) {
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
          <TableHead className={HEAD}>{t("panelAccess.col.target")}</TableHead>
          <TableHead className="sr-only">{t("panelAccess.revoke")}</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {holders.map((h) => {
          const grantId = String(h.grant?.id ?? "");
          const name = holderName(h);
          return (
            <TableRow key={grantId}>
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
              <TableCell className="max-w-32">
                <div className="truncate font-mono text-xs" title={h.grant?.target}>
                  {h.grant?.target}
                </div>
                {h.grant?.orphaned && (
                  <Badge variant="outline" title={t("panelAccess.orphanedHint")}>
                    {t("panelAccess.orphaned")}
                  </Badge>
                )}
              </TableCell>
              <TableCell className="text-right">
                {/* Offered on every row: which targets the caller may revoke is their
                    delegation, which the cabinet cannot read, so the plane says no. An icon,
                    not the word: the panel is 340px and the word is in the confirmation. */}
                <Button type="button" icon variant="ghost" size="sm" aria-label={t("panelAccess.revoke")} title={t("panelAccess.revoke")} disabled={busyGrantId !== null} onClick={() => onRevoke(h)}>
                  {busyGrantId === grantId ? <Spinner /> : <UserMinus className="size-4" aria-hidden />}
                </Button>
              </TableCell>
            </TableRow>
          );
        })}
      </TableBody>
    </Table>
  );
}
