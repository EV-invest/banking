"use client";

// The subscribe form. Extracted from the invest list when the product page arrived: a
// subscription has to behave identically wherever it is initiated from, and the surest
// way to guarantee that is one implementation.
//
// No `onDone`: `submitSubscribe` names what it moved (see `entities/fund/model/fund-resource`),
// so the page around this — and Home, and Wallet — refreshes itself.
//
// Everything the hub would refuse is stated before the click (#396): the tier gate takes
// the form's place, the balance sits on the label with a Max, and a shortfall names the
// top-up as the way out — the same shape the withdraw screen already had (#220, #315).

import { useAnalytics } from "@evinvest/analytics/react";
import { useLocale, useT } from "@evinvest/i18n/react";
import { Sparkles, TriangleAlert } from "lucide-react";
import { useState } from "react";

import { Alert, AlertDescription, AlertTitle, Button, Skeleton, Spinner } from "@evinvest/uikit";

import { hasHoldings } from "@/entities/fund/lib/holdings";
import { positionsResource, submitSubscribe } from "@/entities/fund/model/fund-resource";
import { walletResource } from "@/entities/wallet/model/wallet-resource";
import { useKycGate, VerificationRequired } from "@/features/kyc";
import type { FundNav } from "@/shared/contracts";
import { errorMessage } from "@/shared/lib/api-client";
import { useResource } from "@/shared/lib/resource";
import { Panel, PanelPresence } from "@/shared/ui/motion";
import { recordFirstSubscription } from "@/views/invest/lib/first-subscription";
import { formatExactUsdt, formatUnits, formatUsdt } from "@/views/invest/lib/format";
import { canSubmit, checkSubscribe } from "@/views/invest/lib/subscribe-check";
import { SubscribeField } from "@/views/invest/ui/subscribe-field";

export function SubscribePanel({ service, nav }: { service: string; nav: FundNav | null }) {
  const t = useT();
  const locale = useLocale();
  const [amount, setAmount] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [done, setDone] = useState<{ units?: string; nav?: string } | null>(null);
  const capture = useAnalytics();
  const { gated, loading: tierLoading } = useKycGate();
  // The same cached balance Home, Wallet and the invest list read. A failed read hides the
  // figure rather than gating on it — see `checkSubscribe`.
  const { data: wallet } = useResource(walletResource);
  const available = wallet?.balance?.available ?? null;

  const check = checkSubscribe({ amount, available, nav });

  const submit = async () => {
    if (submitting || !canSubmit(check)) return;
    setSubmitting(true);
    setError(null);
    setDone(null);
    // Read BEFORE the submit: the mutation refreshes the positions, and afterwards the
    // holding this subscription just opened would be the "previous" one.
    const held = hasHoldings(positionsResource.peek());
    try {
      const receipt = await submitSubscribe({ service, amount });
      setDone({ units: receipt.units, nav: receipt.nav });
      setAmount("");
      recordFirstSubscription(capture, service, held);
    } catch (e) {
      // The error itself: `errorMessage` resolves its `code` in the reader's locale.
      setError(e);
    } finally {
      setSubmitting(false);
    }
  };

  // A skeleton, not a verdict, until the tier is known — the same rule the wallet applies.
  if (tierLoading) return <Skeleton className="h-36 w-full rounded-lg" />;
  if (gated) return <VerificationRequired title={t("invest.subscribeVerifyTitle")} description={t("invest.subscribeVerifyBody")} />;

  return (
    <div className="space-y-3 rounded-lg border border-border bg-secondary p-4">
      {/* The receipt and the failure occupy the same slot and replace one another, so
          they share a presence boundary: retrying after an error swaps the panel in
          place instead of collapsing the form and re-expanding it. */}
      <PanelPresence>
        {done && (
          <Panel key="receipt" from="bottom">
            <Alert>
              <Sparkles className="size-4 text-positive" />
              <AlertTitle>{t("invest.subscribeReceived")}</AlertTitle>
              <AlertDescription>{t("invest.subscribeReceiptBody", { n: Number(done.units ?? 0), units: formatUnits(done.units, locale), nav: formatUsdt(done.nav, locale) })}</AlertDescription>
            </Alert>
          </Panel>
        )}
        {!!error && (
          <Panel key="error" from="bottom">
            <Alert variant="destructive">
              <TriangleAlert className="size-4" />
              <AlertTitle>{t("invest.subscribeFailed")}</AlertTitle>
              <AlertDescription>{errorMessage(error, t)}</AlertDescription>
            </Alert>
          </Panel>
        )}
      </PanelPresence>

      <SubscribeField
        amount={amount}
        available={available}
        check={check}
        nav={nav}
        onChange={setAmount}
        action={
          <Button type="button" className="tabular-nums" disabled={submitting || !canSubmit(check)} onClick={submit}>
            {submitting ? <Spinner aria-hidden /> : <Sparkles className="size-4" />}
            {/* Names the outcome once there is one to name, and names it EXACTLY: the check
                runs at 18 dp, so a rounding formatter could read "0.00" over an amount
                that still buys a fraction of a unit and submits. */}
            {check.preview !== null ? t("invest.investAmount", { amount: formatExactUsdt(amount, locale) }) : t("invest.subscribe")}
          </Button>
        }
      />
    </div>
  );
}
