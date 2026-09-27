"use client";

// Who holds one allocation, in a table cell: the largest holders inline, the rest folded
// into a count — a product can have hundreds. A person is their plane id (the directory
// has the name); the `fee` allocation holding a product's fee class is named, and never
// looked up as a person. The same marks as the cap table, so the two read alike.

import { useLocale, useT } from "@evinvest/i18n/react";

import type { UnitHolding } from "@/shared/contracts/admin";
import { cn } from "@/shared/lib/cn";
import { formatUnits } from "@/shared/lib/money";
import { holderLabel } from "@/views/admin/lib/format";
import { HolderMark } from "@/views/admin/ui/holders-table";

const SHOWN_HOLDERS = 3;

export function HoldersCell({ holders }: { holders: UnitHolding[] }) {
  const t = useT();
  const locale = useLocale();
  if (holders.length === 0) return <span className="text-xs text-ink-soft">{t("admin.treasury.allocations.noHolders")}</span>;
  const shown = holders.slice(0, SHOWN_HOLDERS);
  const rest = holders.length - shown.length;
  return (
    <ul className="space-y-0.5 text-xs">
      {shown.map((line) => {
        const person = line.holder.kind === "user";
        return (
          <li key={`${line.holder.kind}:${line.holder.id}`} className="flex items-center gap-1.5 tabular-nums" title={line.holder.id}>
            <HolderMark person={person} />
            <span className={cn("max-w-40 truncate", person ? "font-mono-tech" : "font-medium")}>{holderLabel(line.holder, t)}</span>
            <span className="text-ink-soft">{formatUnits(line.units, locale)}</span>
          </li>
        );
      })}
      {rest > 0 && <li className="text-ink-soft">{t("admin.treasury.allocations.moreHolders", { n: rest })}</li>}
    </ul>
  );
}
