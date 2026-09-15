"use client";

// One product's settled supply, split by who holds it. Three classes and a total, each
// with its share of the outstanding figure — the question an operator asks before
// issuing more to the company is "how much of this fund is already ours?".

import { PieChart } from "lucide-react";

import { useT } from "@evinvest/i18n/react";
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle, Progress } from "@evinvest/uikit";

import type { UnitHolders } from "@/shared/contracts/admin";
import { formatUnits, isZero, shareBps } from "@/shared/lib/money";
import { pct } from "@/shared/lib/rate";

export function HoldersTable({ holders }: { holders: UnitHolders }) {
  const t = useT();
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
              {t("admin.alloc.holders.queued")} · {formatUnits(holders.queued_units)}
            </p>
          </EmptyContent>
        )}
      </Empty>
    );
  }

  const classes: { key: string; label: string; units: string }[] = [
    { key: "company", label: t("admin.alloc.holders.company"), units: holders.company_units },
    { key: "fee", label: t("admin.alloc.holders.fee"), units: holders.fee_units },
    { key: "investors", label: t("admin.alloc.holders.investors"), units: holders.investor_units },
  ];

  return (
    <dl className="space-y-3 text-sm">
      <div className="flex items-baseline justify-between gap-3 border-b border-border pb-2">
        <dt className="text-muted-foreground">{t("admin.alloc.holders.outstanding")}</dt>
        <dd className="font-semibold tabular-nums">{formatUnits(outstanding)}</dd>
      </div>
      {classes.map((c) => {
        const bps = shareBps(c.units, outstanding);
        return (
          <div key={c.key} className="space-y-1.5">
            <div className="flex items-baseline justify-between gap-3">
              <dt className="text-muted-foreground">{c.label}</dt>
              <dd className="tabular-nums">
                <span className="font-medium">{pct(bps)}</span>
                <span className="text-xs text-muted-foreground"> · {formatUnits(c.units)}</span>
              </dd>
            </div>
            <Progress value={bps / 100} className="h-1.5" aria-hidden />
          </div>
        );
      })}
      {!isZero(holders.queued_units) && (
        // No share bar: a queued mint is not part of the outstanding figure it would be
        // measured against, so a percentage here would be a lie either way.
        <div className="flex items-baseline justify-between gap-3 border-t border-border pt-2">
          <dt className="text-muted-foreground">{t("admin.alloc.holders.queued")}</dt>
          <dd className="tabular-nums text-muted-foreground">{formatUnits(holders.queued_units)}</dd>
        </div>
      )}
    </dl>
  );
}
