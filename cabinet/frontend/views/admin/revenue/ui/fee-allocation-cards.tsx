"use client";

// The `fee` allocation's four figures: cash available (the one an owner acts on, so it
// carries the colour), cash posted with what is reserved, the supply, and the price. All
// read off the allocation's own accounts — none is a difference of the others.

import { useLocale, useT } from "@evinvest/i18n/react";
import { Card, CardContent, Skeleton } from "@evinvest/uikit";

import type { AllocationTreasury } from "@/shared/contracts/admin";
import { formatNav, formatUnits } from "@/shared/lib/money";
import { hasUnixStamp } from "@/shared/lib/unix-stamp";
import { ago } from "@/views/admin/lib/format";
import { MoneyCard } from "@/views/admin/revenue/ui/money-card";

export function FeeAllocationCards({ fee, loading, unavailable }: { fee: AllocationTreasury | null; loading: boolean; unavailable: boolean }) {
  const t = useT();
  const locale = useLocale();
  return (
    <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
      <MoneyCard label={t("admin.revenue.cashAvailable")} value={fee?.claim.available} hint={t("admin.revenue.cashAvailableHint")} loading={loading} unavailable={unavailable} emphasis />
      <MoneyCard label={t("admin.revenue.cashPosted")} value={fee?.claim.posted} hint={t("admin.revenue.cashPostedHint")} loading={loading} unavailable={unavailable} />
      <FigureCard label={t("admin.alloc.holders.outstanding")} value={fee ? formatUnits(fee.units_outstanding, locale) : undefined} hint={t("admin.revenue.unitsHint")} loading={loading} unavailable={unavailable} />
      <FigureCard
        label="NAV"
        value={fee ? formatNav(fee.nav, locale) : undefined}
        // `"0"` is "never marked" — priced at seed — not a mark at the epoch.
        hint={fee && hasUnixStamp(fee.nav_posted_at) ? t("admin.revenue.navPosted", { ago: ago(fee.nav_posted_at, t) }) : t("admin.treasury.allocations.unmarked")}
        loading={loading}
        unavailable={unavailable}
      />
    </div>
  );
}

/** A non-money figure in the money cards' frame: units and a price, already formatted by
 *  their own policy, so the card does not run them through `formatUsd`. */
function FigureCard({ label, value, hint, loading, unavailable }: { label: string; value: string | undefined; hint: string; loading: boolean; unavailable: boolean }) {
  return (
    <Card>
      <CardContent className="space-y-1 py-5">
        <p className="text-xs text-ink-soft">{label}</p>
        {loading ? <Skeleton className="mt-1 h-8 w-28" /> : <p className={unavailable ? "text-3xl font-semibold tabular-nums text-ink-soft" : "text-3xl font-semibold tabular-nums"}>{unavailable ? "—" : value}</p>}
        {!loading && !unavailable && <p className="text-xs text-ink-soft">{hint}</p>}
      </CardContent>
    </Card>
  );
}
