"use client";

// Admin console — the platform's OWN money: the reserved `fee` allocation (#245).
//
// What used to be three figures off one claim is an allocation like any other: a cash
// claim credited by retained withdrawal fees and settled 2-and-20, a supply of units, a
// NAV, and holders — the people the owners have seated on it. The screen's whole job is
// still to make one distinction unmistakable: this is never client money and never the
// `fund` allocation, which are separate claims this surface cannot reach.
//
// Nothing pays revenue out from here. A holder redeems, or the owners approve a payment
// out of `service:fee` on the Payments screen; the one write here is a PROPOSAL — seating
// a person — and the payout list below is history from before the kind retired.

import { useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Skeleton } from "@evinvest/uikit";

import { cancelRevenuePayout } from "@/entities/admin/api/admin-client";
import { fundRevenueResource, revenuePayoutsResource } from "@/entities/admin/model/admin-resource";
import { errorMessage } from "@/shared/lib/api-client";
import { TAG } from "@/shared/lib/cache-tags";
import { revalidateTag, useResource } from "@/shared/lib/resource";
import { Settled, StaggerItem } from "@/shared/ui/motion";
import { ResourceError } from "@/shared/ui/resource-error";
import { FeeAllocationCards } from "@/views/admin/revenue/ui/fee-allocation-cards";
import { HolderGrantForm } from "@/views/admin/revenue/ui/holder-grant-form";
import { PayoutHistory } from "@/views/admin/revenue/ui/payout-history";
import { WhereNext } from "@/views/admin/revenue/ui/where-next";
import { HoldersTable } from "@/views/admin/ui/holders-table";
import { AdminHeader, AdminScreen } from "@/views/admin/ui/shell";

export function RevenueView() {
  const t = useT();
  const revenue = useResource(fundRevenueResource);
  const payouts = useResource(revenuePayoutsResource);
  const [busy, setBusy] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);

  const fee = revenue.data ?? null;
  const history = payouts.data?.withdrawals ?? null;
  const failed = !fee && Boolean(revenue.error);
  const error = actionError ?? (failed ? errorMessage(revenue.error, t) : null);

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
        <p className="text-xs font-semibold uppercase tracking-widest text-ink-soft">{t("admin.revenue.earned")}</p>
        <FeeAllocationCards fee={fee} loading={!fee && !failed} unavailable={failed} />
        <p className="max-w-3xl text-xs text-ink-soft">{t("admin.revenue.ownMoneyNote")}</p>
      </StaggerItem>

      <StaggerItem as="section" className="space-y-3">
        <p className="text-xs font-semibold uppercase tracking-widest text-ink-soft">{t("admin.alloc.holders.title")}</p>
        <div className="grid gap-4 lg:grid-cols-2">
          <Settled loading={!fee && !failed} skeleton={<Skeleton className="h-24 w-full" />}>
            {fee && <HoldersTable holders={fee.holders} outstanding={fee.units_outstanding} />}
          </Settled>
          <HolderGrantForm allocation="fee" />
        </div>
      </StaggerItem>

      <StaggerItem as="section" className="space-y-3">
        <p className="text-xs font-semibold uppercase tracking-widest text-ink-soft">{t("admin.revenue.whereNext")}</p>
        <WhereNext />
      </StaggerItem>

      <StaggerItem as="section" className="space-y-3">
        <p className="text-xs font-semibold uppercase tracking-widest text-ink-soft">{t("admin.revenue.payouts")}</p>
        <PayoutHistory history={history} error={history ? null : payouts.error} onRetry={() => void payouts.refresh()} busy={busy} onCancel={(id) => void cancel(id)} />
        <p className="max-w-3xl text-xs text-ink-soft">{t("admin.revenue.footnote")}</p>
      </StaggerItem>
    </AdminScreen>
  );
}
