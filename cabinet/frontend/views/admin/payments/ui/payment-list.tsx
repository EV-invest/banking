"use client";

// Every order the caller may see, newest first as the plane returns them, narrowed by
// state server-side: a filtered list is a different question, and its own cache entry.

import { ArrowLeftRight } from "lucide-react";
import { useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Card, CardContent, Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle, Select, SelectContent, SelectItem, SelectTrigger, Skeleton } from "@evinvest/uikit";

import { paymentStateLabel } from "@/entities/payment/lib/format";
import { cancelPayment, paymentsResource } from "@/entities/payment/model/payment-resource";
import { errorMessage } from "@/shared/lib/api-client";
import { useResource } from "@/shared/lib/resource";
import { Settled } from "@/shared/ui/motion";
import { ResourceError } from "@/shared/ui/resource-error";
import { PaymentRow } from "@/views/admin/payments/ui/payment-row";

/** The lifecycle as the plane spells it, in the order an order passes through it. */
const STATES = ["pending", "approved", "executed", "execution_failed", "rejected", "expired", "cancelled"] as const;

export function PaymentList() {
  const t = useT();
  const [state, setState] = useState("");
  const list = useResource(paymentsResource, state || undefined);
  const items = list.data?.items ?? null;
  const [busy, setBusy] = useState<string | null>(null);
  const [actionError, setActionError] = useState<unknown>(null);
  const error = actionError ?? (items || !list.error ? null : list.error);

  const cancel = async (id: string) => {
    setBusy(id);
    setActionError(null);
    try {
      await cancelPayment(id);
    } catch (cause) {
      setActionError(cause);
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-3">
        <span className="text-xs font-semibold uppercase tracking-widest text-muted-foreground">{t("admin.payments.list")}</span>
        <div className="ml-auto inline-flex items-center gap-2 text-sm">
          <span className="text-muted-foreground">{t("admin.col.state")}:</span>
          <Select value={state} onValueChange={setState}>
            <SelectTrigger size="sm" className="border-border bg-main-surface">
              <span className="truncate">{state ? paymentStateLabel(state, t) : t("ui.all")}</span>
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="">{t("ui.all")}</SelectItem>
              {STATES.map((s) => (
                <SelectItem key={s} value={s}>
                  {paymentStateLabel(s, t)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>

      {error !== null && <ResourceError message={errorMessage(error, t)} />}

      {/* No card while the read has failed with nothing to show: a `Settled` with null
          children would leave an empty box under the error. */}
      {!items && list.error ? null : (
        <Card>
          <CardContent className="p-0">
            <Settled loading={!items && !list.error} skeleton={<Skeleton className="m-6 h-24" />}>
              {!items ? null : items.length === 0 ? (
                <div className="p-8">
                  <Empty className="border md:p-6">
                    <EmptyHeader>
                      <EmptyMedia variant="icon">
                        <ArrowLeftRight />
                      </EmptyMedia>
                      <EmptyTitle>{state ? t("admin.payments.noneInState") : t("admin.payments.none")}</EmptyTitle>
                      <EmptyDescription>{t("admin.payments.noneHint")}</EmptyDescription>
                    </EmptyHeader>
                  </Empty>
                </div>
              ) : (
                // Seven columns do not fit a phone: the table scrolls inside its own box.
                <div className="overflow-x-auto">
                  <table className="w-full min-w-200 text-sm">
                    <thead>
                      <tr className="border-b border-border text-left text-xs uppercase tracking-wide text-muted-foreground">
                        <th className="px-5 py-3 font-medium">{t("admin.payments.col.opened")}</th>
                        <th className="px-5 py-3 font-medium">{t("admin.payments.col.ends")}</th>
                        <th className="px-5 py-3 font-medium">{t("admin.payments.col.amountUsdt")}</th>
                        <th className="px-5 py-3 font-medium">{t("admin.payments.tier")}</th>
                        <th className="px-5 py-3 font-medium">{t("admin.payments.col.approval")}</th>
                        <th className="px-5 py-3 font-medium">{t("admin.col.state")}</th>
                        <th className="px-5 py-3 text-right font-medium">{t("admin.col.actions")}</th>
                      </tr>
                    </thead>
                    <tbody className="divide-y divide-border">
                      {items.map((payment) => (
                        <PaymentRow key={payment.id} payment={payment} busy={busy === payment.id} onCancel={() => void cancel(payment.id)} />
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
            </Settled>
          </CardContent>
        </Card>
      )}
    </div>
  );
}
