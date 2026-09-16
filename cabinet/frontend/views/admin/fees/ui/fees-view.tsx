"use client";

// Admin console — what each fund charges, and collecting it.
//
// Two things happen here and they are deliberately kept apart. The LEFT column prices a
// product: the terms in force, the change on its way, and the form that schedules the
// next one — terms an investor reads before they subscribe, and changing them changes what
// people pay, which is why a change is versioned, noticed and sometimes voted on rather
// than written in place (docs/FEES.md § Changing the terms). The RIGHT column collects what
// those terms have already earned: units the sweeper clawed back, converted to cash in one
// bulk settlement per period. Pricing is a decision; collecting is bookkeeping.
//
// What this screen does NOT do is pay the money out. Once settled, fee cash lands in the
// `fee` claim, which is exactly what `Fund revenue` withdraws on-chain — same account, an
// existing pipeline with its own rail liquidity and dispatch gates. Duplicating a payout
// form here would give an operator two doors to the same money.

import { Landmark, ShieldAlert } from "lucide-react";
import { useCallback, useMemo, useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Button, Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle, Skeleton } from "@evinvest/uikit";

import { adminAllocationsResource, feePoliciesResource } from "@/entities/admin/model/admin-resource";
import type { FeePolicyChange } from "@/shared/contracts/admin";
import { RequestError } from "@/shared/lib/api-client";
import { useResource } from "@/shared/lib/resource";
import { Link } from "@/shared/ui/cabinet-link";
import { StaggerItem } from "@/shared/ui/motion";
import { ResourceError } from "@/shared/ui/resource-error";
import { isPendingChange } from "@/views/admin/fees/lib/format";
import { AssessmentsCard } from "@/views/admin/fees/ui/assessments-card";
import { ChangeHistory } from "@/views/admin/fees/ui/change-history";
import { CollectCard } from "@/views/admin/fees/ui/collect-card";
import { FundPicker } from "@/views/admin/fees/ui/fund-picker";
import { PendingCard } from "@/views/admin/fees/ui/pending-card";
import { PolicyCard } from "@/views/admin/fees/ui/policy-card";
import { ScheduledReceipt } from "@/views/admin/fees/ui/scheduled-receipt";
import { AdminHeader, AdminScreen } from "@/views/admin/ui/shell";

const isForbidden = (error: unknown): boolean => error instanceof RequestError && error.status === 403;

export function FeesView() {
  const t = useT();
  const catalog = useResource(adminAllocationsResource);
  const policies = useResource(feePoliciesResource);
  const [service, setService] = useState("");
  /** What the operator just scheduled, until dismissed, cancelled or another fund is picked.
   *  Cancelling clears it explicitly rather than by watching `pending` vanish: right after
   *  scheduling, the receipt exists BEFORE the re-read brings the new pending change in. */
  const [receipt, setReceipt] = useState<FeePolicyChange | null>(null);

  /** The fund whose pending change was just cancelled: its terms card takes focus once the
   *  re-read has removed the pending card, whose button the focus was on. */
  const [cancelledIn, setCancelledIn] = useState<string | null>(null);
  const onTermsFocused = useCallback(() => setCancelledIn(null), []);

  const funds = catalog.data?.allocations ?? [];
  // The first fund is the default so the screen is useful without a click.
  const selected = service || funds[0]?.service || "";
  const byService = useMemo(() => new Map((policies.data?.policies ?? []).map((p) => [p.service, p])), [policies.data]);
  const policy = byService.get(selected) ?? null;
  const pending = policy?.pending && isPendingChange(policy.pending.state) ? policy.pending : null;

  const loading = catalog.isLoading || policies.isLoading;
  // A read that failed with nothing to show is reported in place of the screen, never
  // rendered through: without the policies every fund would read "No fee" and the form
  // would open on the house default with a "Start charging" button — a claim about
  // pricing derived from nothing at all.
  const catalogFailed = !catalog.data && Boolean(catalog.error);
  const policiesFailed = !policies.data && Boolean(policies.error);
  const failed = catalogFailed ? catalog : policiesFailed ? policies : null;
  // A verdict, not a failure: `/api/admin/fees/*` admits only admins and owners, so an
  // operator who typed the URL (the rail no longer offers it — banking#269) gets told whose
  // screen this is instead of a retry button over a 403 that will never turn into a 200.
  // Decided ahead of the cards below, each of which would otherwise open its own refused
  // read against the same gate.
  const forbidden = isForbidden(catalog.error) || isForbidden(policies.error);

  return (
    <AdminScreen className="space-y-6">
      <AdminHeader eyebrow={t("nav.fees")} title={t("admin.fees.title")} subtitle={t("admin.fees.subtitle")} />

      {forbidden ? (
        <StaggerItem as={Empty} className="border">
          <EmptyHeader>
            <EmptyMedia variant="icon">
              <ShieldAlert />
            </EmptyMedia>
            <EmptyTitle>{t("admin.fees.forbidden.title")}</EmptyTitle>
            <EmptyDescription>{t("admin.fees.forbidden.body")}</EmptyDescription>
          </EmptyHeader>
          <EmptyContent>
            <Button asChild variant="outline">
              <Link href="/admin/users">{t("nav.users")}</Link>
            </Button>
          </EmptyContent>
        </StaggerItem>
      ) : failed ? (
        <ResourceError error={failed.error} onRetry={() => void failed.refresh()} retrying={failed.isValidating} />
      ) : loading ? (
        <StaggerItem>
          <Skeleton className="h-64 w-full" />
        </StaggerItem>
      ) : funds.length === 0 ? (
        <StaggerItem as={Empty} className="border">
          <EmptyHeader>
            <EmptyMedia variant="icon">
              <Landmark />
            </EmptyMedia>
            <EmptyTitle>{t("admin.fees.noFunds")}</EmptyTitle>
            <EmptyDescription>{t("admin.fees.noFundsHint")}</EmptyDescription>
          </EmptyHeader>
        </StaggerItem>
      ) : (
        <>
          <FundPicker funds={funds.map((f) => ({ service: f.service, title: f.title }))} selected={selected} onSelect={setService} policies={byService} />
          <StaggerItem className="grid gap-5 lg:grid-cols-2">
            <div className="space-y-5">
              {receipt && receipt.service === selected && <ScheduledReceipt change={receipt} onDismiss={() => setReceipt(null)} />}
              {pending && (
                <PendingCard
                  key={pending.id}
                  change={pending}
                  onCancelled={() => {
                    setReceipt(null);
                    setCancelledIn(selected);
                  }}
                />
              )}
              {/* Keyed on the fund AND the pending change: a cancel or a promotion reseeds
                  the draft from the terms that are now in force. */}
              <PolicyCard
                key={`${selected}:${pending?.id ?? policy?.version ?? 0}`}
                service={selected}
                policy={policy}
                onScheduled={setReceipt}
                focusTitle={cancelledIn === selected && pending === null}
                onTitleFocused={onTermsFocused}
              />
            </div>
            <CollectCard service={selected} />
          </StaggerItem>
          <StaggerItem>
            <ChangeHistory service={selected} />
          </StaggerItem>
          <StaggerItem>
            <AssessmentsCard service={selected} />
          </StaggerItem>
        </>
      )}
    </AdminScreen>
  );
}
