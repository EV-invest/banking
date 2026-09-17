"use client";

// Who the claims on custody belong to, through units (#245): every allocation the hub
// knows — the products and the hidden `fee` / `fund` — with its cash claim, supply, price
// and holders. Read, never derived: each figure is the allocation's own ledger account,
// and Σ claims + `held_by_users` = custody is the hub's invariant, not this table's sum.

import { Layers } from "lucide-react";

import { useLocale, useT } from "@evinvest/i18n/react";
import { Card, CardContent, Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle, Skeleton } from "@evinvest/uikit";

import type { AllocationTreasury, UnitHolding } from "@/shared/contracts/admin";
import { cn } from "@/shared/lib/cn";
import { formatNav, formatUnits, formatUsd } from "@/shared/lib/money";
import { hasUnixStamp } from "@/shared/lib/unix-stamp";
import { Settled } from "@/shared/ui/motion";
import { accessLabel, accessTone } from "@/views/admin/lib/access";
import { ago, holderLabel } from "@/views/admin/lib/format";

/** Holders shown inline before the rest fold into a count — a product can have hundreds. */
const SHOWN_HOLDERS = 3;

export function TreasuryAllocations({ allocations }: { allocations: AllocationTreasury[] | null }) {
  const t = useT();
  return (
    <Card>
      <CardContent className="p-0">
        <Settled loading={!allocations} skeleton={<Skeleton className="m-6 h-24" />}>
          {!allocations ? null : allocations.length === 0 ? (
            <div className="p-8">
              <Empty className="border md:p-6">
                <EmptyHeader>
                  <EmptyMedia variant="icon">
                    <Layers />
                  </EmptyMedia>
                  <EmptyTitle>{t("admin.treasury.allocations.empty")}</EmptyTitle>
                  <EmptyDescription>{t("admin.treasury.allocations.emptyHint")}</EmptyDescription>
                </EmptyHeader>
              </Empty>
            </div>
          ) : (
            // Six columns do not fit a phone: the table scrolls inside its own box.
            <div className="overflow-x-auto">
              <table className="w-full min-w-160 text-sm">
                <thead>
                  {/* i18n-max: 14 per header — a long header widens the scroll, not a cell. */}
                  <tr className="border-b border-border text-left text-xs uppercase tracking-wide text-ink-soft">
                    <th className="px-5 py-3 font-medium">{t("admin.treasury.allocations.col.allocation")}</th>
                    <th className="px-5 py-3 font-medium">{t("admin.alloc.col.access")}</th>
                    <th className="px-5 py-3 text-right font-medium">{t("admin.treasury.allocations.col.claim")}</th>
                    <th className="px-5 py-3 text-right font-medium">{t("admin.alloc.holders.outstanding")}</th>
                    <th className="px-5 py-3 text-right font-medium">NAV</th>
                    <th className="px-5 py-3 font-medium">{t("admin.alloc.holders.title")}</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-border">
                  {allocations.map((a) => (
                    <AllocationRow key={a.service} allocation={a} />
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </Settled>
      </CardContent>
    </Card>
  );
}

function AllocationRow({ allocation: a }: { allocation: AllocationTreasury }) {
  const t = useT();
  const locale = useLocale();
  return (
    <tr>
      <td className="px-5 py-3">
        <p className="font-medium">{a.title || a.service}</p>
        <p className="font-mono-tech text-xs text-ink-soft">{a.service}</p>
      </td>
      <td className="px-5 py-3">
        <span className={cn("inline-flex whitespace-nowrap rounded-full border px-2 py-0.5 text-xs font-medium", accessTone(a.access))}>{accessLabel(a.access, t)}</span>
      </td>
      <td className="px-5 py-3 text-right tabular-nums">
        <p>{formatUsd(a.claim.available, locale)}</p>
        {/* Posted and reserved beside the spendable figure: an operator judging a payment
            out of the claim needs the reserved part, not only what is left. */}
        <p className="text-xs text-ink-soft">{t("admin.treasury.allocations.claimDetail", { posted: formatUsd(a.claim.posted, locale), reserved: formatUsd(a.claim.reserved, locale) })}</p>
      </td>
      <td className="px-5 py-3 text-right tabular-nums">{formatUnits(a.units_outstanding, locale)}</td>
      <td className="px-5 py-3 text-right tabular-nums">
        <p>{formatNav(a.nav, locale)}</p>
        {/* `"0"` is "never marked" — the hub prices it at seed — not a mark at the epoch. */}
        <p className="text-xs text-ink-soft">{hasUnixStamp(a.nav_posted_at) ? ago(a.nav_posted_at, t) : t("admin.treasury.allocations.unmarked")}</p>
      </td>
      <td className="px-5 py-3">
        <HoldersCell holders={a.holders} />
      </td>
    </tr>
  );
}

function HoldersCell({ holders }: { holders: UnitHolding[] }) {
  const t = useT();
  const locale = useLocale();
  if (holders.length === 0) return <span className="text-xs text-ink-soft">{t("admin.treasury.allocations.noHolders")}</span>;
  const shown = holders.slice(0, SHOWN_HOLDERS);
  const rest = holders.length - shown.length;
  return (
    <ul className="space-y-0.5 text-xs">
      {shown.map((line) => (
        <li key={`${line.holder.kind}:${line.holder.id}`} className="flex items-baseline gap-1.5 tabular-nums" title={line.holder.id}>
          {/* A person is their plane id (the directory has the name); the `fee` allocation
              holding a product's fee class is named, and never looked up as a person. */}
          <span className={cn("max-w-40 truncate", line.holder.kind === "user" ? "font-mono-tech" : "font-medium")}>{holderLabel(line.holder, t)}</span>
          <span className="text-ink-soft">{formatUnits(line.units, locale)}</span>
        </li>
      ))}
      {rest > 0 && <li className="text-ink-soft">{t("admin.treasury.allocations.moreHolders", { n: rest })}</li>}
    </ul>
  );
}
