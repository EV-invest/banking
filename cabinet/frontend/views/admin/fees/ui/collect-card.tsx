"use client";

// Accumulated fee units and the one button that turns them into cash.
//
// Collecting is bookkeeping, kept apart from pricing (the policy card): the terms decide
// what people pay, this converts what those terms have already earned. It does NOT pay the
// money out — once settled, fee cash lands in the `fee` claim, which is exactly what
// `Fund revenue` withdraws on-chain, and a second door to the same money would be a bug.

import { Loader2 } from "lucide-react";
import { useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Button, Card, CardContent, Skeleton } from "@evinvest/uikit";

import { settleFeeShares } from "@/entities/admin/api/admin-client";
import { feeSharesResource } from "@/entities/admin/model/admin-resource";
import { errorMessage } from "@/shared/lib/api-client";
import { TAG } from "@/shared/lib/cache-tags";
import { revalidateTag, useResource } from "@/shared/lib/resource";
import { ResourceError } from "@/shared/ui/resource-error";
import { Row } from "@/views/admin/fees/ui/fields";
import { formatUnits, formatUsdt } from "@/views/admin/lib/format";

export function CollectCard({ service }: { service: string }) {
  const t = useT();
  const shares = useResource(feeSharesResource, service);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const [done, setDone] = useState<string | null>(null);

  const data = shares.data ?? null;
  const failed = !data && Boolean(shares.error);
  // Decided only from figures that arrived: "nothing to settle" on a read that failed
  // would tell the operator the fund has earned nothing when the truth is unknown.
  const nothing = data !== null && Number(data.units) <= 0;

  async function settle() {
    setBusy(true);
    setProblem(null);
    setDone(null);
    try {
      // Empty units settles the whole balance — the ordinary end-of-period call, and the
      // only one worth a button. A partial settlement is a rare, deliberate act better
      // done against the API than offered as a field nobody needs.
      const settlement = await settleFeeShares({ service, units: "" });
      // The settle moves the fee units AND the revenue figure the payout screen reads.
      revalidateTag(TAG.adminFees, TAG.adminRevenue);
      // Fee cash is USDT, and `formatUsdt` carries no symbol: the unit rides in the value so
      // the sentence still names it, the way the "worth" row below does.
      setDone(t("admin.fees.settledAtNav", { cash: `${formatUsdt(settlement.cash)} USDT`, nav: settlement.nav }));
    } catch (e) {
      setProblem(e instanceof Error ? errorMessage(e, t) : t("err.feeSettle"));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card className="h-fit">
      <CardContent className="space-y-4 py-6">
        <div className="space-y-1">
          <p className="text-sm font-semibold">{t("admin.fees.collected")}</p>
          <p className="text-xs text-ink-soft">{t("admin.fees.collectedSub")}</p>
        </div>

        {shares.isLoading ? (
          <Skeleton className="h-16 w-full" />
        ) : failed ? (
          <ResourceError error={shares.error} onRetry={() => void shares.refresh()} retrying={shares.isValidating} />
        ) : (
          <dl className="space-y-2.5 text-sm">
            <Row label={t("admin.fees.unitsHeld")} value={formatUnits(data?.units)} />
            <Row label={t("admin.fees.worthAtNav")} value={`${formatUsdt(data?.value)} USDT`} />
          </dl>
        )}

        <p className="text-xs text-ink-soft">{t("admin.fees.settleNote")}</p>

        {problem && <p className="text-xs text-destructive">{problem}</p>}
        {/* Two independently complete sentences, so the settlement line and the pointer to
            the payout screen stay separate keys; the screen's own name is interpolated so it
            tracks whatever the nav calls it. The emphasis on that name is the one casualty
            of keeping the sentence whole for translators. */}
        {done && !problem && <p className="text-xs text-main-accent-t2">{`${done} ${t("admin.fees.withdrawableFrom", { screen: t("nav.revenue") })}`}</p>}

        <Button type="button" variant="outline" onClick={settle} disabled={busy || data === null || nothing}>
          {busy && <Loader2 className="size-4 animate-spin" />}
          {t("admin.fees.settleAll")}
        </Button>
        {nothing && <p className="text-xs text-ink-soft">{t("admin.fees.nothingToSettle")}</p>}
      </CardContent>
    </Card>
  );
}
