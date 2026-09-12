"use client";

// Admin console — the fund's OWN money: what it earned, and where it went.
//
// The screen's whole job is to make one distinction unmistakable, because it is the one
// an operator could otherwise get wrong with real consequences: this page concerns company
// revenue (retained withdrawal fees + the settled 2-and-20), never client balances and
// never the fund's seed capital. Those are separate ledger claims that this surface
// cannot reach at all.
//
// Statistics only. The "propose a payout" form that used to sit here is one case of a
// payment order now — fund revenue to an address — and lives on the Payments screen with
// every other case, where the owners' consilium authorises it the same way. This screen
// links there and to the treasury, and keeps the history of payouts that executed.

import { useState } from "react";

import { useT } from "@evinvest/i18n/react";

import { cancelRevenuePayout } from "@/entities/admin/api/admin-client";
import { fundRevenueResource, revenuePayoutsResource } from "@/entities/admin/model/admin-resource";
import { errorMessage } from "@/shared/lib/api-client";
import { TAG } from "@/shared/lib/cache-tags";
import { revalidateTag, useResource } from "@/shared/lib/resource";
import { StaggerItem } from "@/shared/ui/motion";
import { ResourceError } from "@/shared/ui/resource-error";
import { MoneyCard } from "@/views/admin/revenue/ui/money-card";
import { PayoutHistory } from "@/views/admin/revenue/ui/payout-history";
import { WhereNext } from "@/views/admin/revenue/ui/where-next";
import { AdminHeader, AdminScreen } from "@/views/admin/ui/shell";

export function RevenueView() {
  const t = useT();
  const revenue = useResource(fundRevenueResource);
  const payouts = useResource(revenuePayoutsResource);
  const [busy, setBusy] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);

  const data = revenue.data ?? null;
  const history = payouts.data?.withdrawals ?? null;
  const error = actionError ?? (data || !revenue.error ? null : errorMessage(revenue.error, t));

  // A cancelled payout releases a claim and leaves the operator withdrawal queue, so it
  // moves three facts, not one. Naming all three keeps the treasury and queue in step.
  const cancel = async (id: string) => {
    setBusy(id);
    setActionError(null);
    try {
      await cancelRevenuePayout(id);
      revalidateTag(TAG.adminRevenue, TAG.adminQueue, TAG.adminTreasury);
      await Promise.all([revenue.refresh(), payouts.refresh()]);
    } catch (e) {
      setActionError(errorMessage(e, t));
    } finally {
      setBusy(null);
    }
  };

  return (
    <AdminScreen className="space-y-8">
      <AdminHeader eyebrow={t("admin.eyebrow.administer")} title={t("nav.revenue")} subtitle={t("admin.revenue.subtitle")} />

      {error && <ResourceError message={error} />}

      <StaggerItem as="section" className="space-y-3">
        <p className="text-xs font-semibold uppercase tracking-widest text-muted-foreground">{t("admin.revenue.earned")}</p>
        <div className="grid gap-4 sm:grid-cols-3">
          <MoneyCard label={t("admin.revenue.earnedTotal")} value={data?.earned} hint={t("admin.revenue.earnedTotalHint")} loading={!data} />
          <MoneyCard label={t("admin.revenue.availableToPayOut")} value={data?.available} hint={t("admin.revenue.availableHint")} loading={!data} emphasis />
          <MoneyCard label={t("admin.revenue.pendingPayout")} value={data?.pending_payout} hint={t("admin.revenue.pendingHint")} loading={!data} />
        </div>
        <p className="max-w-3xl text-xs text-muted-foreground">{t("admin.revenue.ownMoneyNote")}</p>
      </StaggerItem>

      <StaggerItem as="section" className="space-y-3">
        <p className="text-xs font-semibold uppercase tracking-widest text-muted-foreground">{t("admin.revenue.whereNext")}</p>
        <WhereNext />
      </StaggerItem>

      <StaggerItem as="section" className="space-y-3">
        <p className="text-xs font-semibold uppercase tracking-widest text-muted-foreground">{t("admin.revenue.payouts")}</p>
        <PayoutHistory history={history} busy={busy} onCancel={(id) => void cancel(id)} />
        <p className="max-w-3xl text-xs text-muted-foreground">{t("admin.revenue.footnote")}</p>
      </StaggerItem>
    </AdminScreen>
  );
}
