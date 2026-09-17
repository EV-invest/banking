"use client";

// One product's cap table: the settled supply and every holder of it, largest first, each
// with its share of the outstanding figure. A holder is a person or the reserved `fee`
// allocation (which holds the product's fee class, #245) — the line says which, because
// the operator's next question differs: "who is this?" for a person, "how much has the
// platform accrued?" for the allocation.

import { Layers, PieChart, User } from "lucide-react";

import { useLocale, useT } from "@evinvest/i18n/react";
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle, Progress } from "@evinvest/uikit";

import type { UnitHolders, UnitHolding } from "@/shared/contracts/admin";
import { formatUnits, isZero, shareBps } from "@/shared/lib/money";
import { pct } from "@/shared/lib/rate";
import { holderLabel } from "@/views/admin/lib/format";

export function HoldersTable({ holders }: { holders: UnitHolders }) {
  const t = useT();
  const locale = useLocale();
  const outstanding = holders.units_outstanding;

  if (shareBps(outstanding, outstanding) === 0) {
    return (
      <Empty className="border">
        <EmptyHeader>
          <EmptyMedia variant="icon">
            <PieChart />
          </EmptyMedia>
          <EmptyTitle>{t("admin.alloc.holders.empty")}</EmptyTitle>
          <EmptyDescription>{t("admin.alloc.holders.emptyHint")}</EmptyDescription>
        </EmptyHeader>
        {!isZero(holders.queued_units) && (
          // The one fact the zero state can report: a mint is recorded and waiting on the
          // relay — the same figure that keeps "Pin cap" disabled next door.
          <EmptyContent>
            <p className="text-sm tabular-nums">
              {t("admin.alloc.holders.queued")} · {formatUnits(holders.queued_units, locale)}
            </p>
          </EmptyContent>
        )}
      </Empty>
    );
  }

  return (
    <dl className="space-y-3 text-sm">
      <div className="flex items-baseline justify-between gap-3 border-b border-border pb-2">
        <dt className="text-ink-soft">{t("admin.alloc.holders.outstanding")}</dt>
        <dd className="font-semibold tabular-nums">{formatUnits(outstanding, locale)}</dd>
      </div>
      {holders.holders.map((line) => (
        <HolderLine key={`${line.holder.kind}:${line.holder.id}`} line={line} outstanding={outstanding} />
      ))}
      {!isZero(holders.queued_units) && (
        // No share bar: a queued mint is not part of the outstanding figure it would be
        // measured against, so a percentage here would be a lie either way.
        <div className="flex items-baseline justify-between gap-3 border-t border-border pt-2">
          <dt className="text-ink-soft">{t("admin.alloc.holders.queued")}</dt>
          <dd className="tabular-nums text-ink-soft">{formatUnits(holders.queued_units, locale)}</dd>
        </div>
      )}
    </dl>
  );
}

function HolderLine({ line, outstanding }: { line: UnitHolding; outstanding: string }) {
  const t = useT();
  const locale = useLocale();
  const bps = shareBps(line.units, outstanding);
  const person = line.holder.kind === "user";
  return (
    <div className="space-y-1.5">
      <div className="flex items-baseline justify-between gap-3">
        <dt className="flex min-w-0 items-center gap-1.5 text-ink-soft" title={line.holder.id}>
          {person ? <User className="size-3.5 shrink-0" aria-label={t("admin.holder.person")} /> : <Layers className="size-3.5 shrink-0" aria-label={t("admin.holder.allocation")} />}
          {/* A person is their plane id here — the cap table carries no email, and the
              directory is where a name lives — so it reads as an identifier, not prose. */}
          <span className={person ? "truncate font-mono-tech text-xs" : "truncate"}>{holderLabel(line.holder, t)}</span>
        </dt>
        <dd className="shrink-0 tabular-nums">
          <span className="font-medium">{pct(bps)}</span>
          <span className="text-xs text-ink-soft"> · {formatUnits(line.units, locale)}</span>
        </dd>
      </div>
      <Progress value={bps / 100} className="h-1.5" aria-hidden />
    </div>
  );
}
