"use client";

// What the form says around its controls: the gate that closes it, and the outcome of
// the last submit. Both stay on screen beside the form — the cabinet mounts no toaster,
// and an answer that names money that moved must not slide away.

import { CheckCircle2, Clock, Lock, TriangleAlert } from "lucide-react";

import { useT } from "@evinvest/i18n/react";
import { Alert, AlertDescription, AlertTitle } from "@evinvest/uikit";

import type { Order } from "@/shared/contracts/book";
import { RequestError, errorMessage } from "@/shared/lib/api-client";
import { Note } from "@/views/invest/ui/atoms";
import { formatUnits, formatUsdt } from "@/views/trade/lib/format";
import { orderStateKey } from "@/views/trade/lib/order-state";

export type Outcome = { order: Order; error?: never } | { error: unknown; order?: never };

/** Why the form is closed, stated before the action. Nothing when it is open. */
export function OrderGate({ closed, locked }: { closed: boolean; locked: boolean }) {
  const t = useT();
  if (!closed && !locked) return null;
  return (
    <div className="px-3 pt-3">
      <Note tone={closed ? "muted" : "amber"}>
        <Lock className="mr-1.5 inline size-3.5 align-text-bottom" />
        {t(closed ? "trade.form.closed" : "trade.form.locked")}
      </Note>
    </div>
  );
}

/** The last submit's answer: the order as the hub recorded it, or why it refused. */
export function OrderOutcome({ outcome }: { outcome: Outcome }) {
  const t = useT();
  if (outcome.order) {
    const order = outcome.order;
    const key = orderStateKey(order);
    const resting = order.state === "open" || order.state === "partially_filled";
    const args = { filled: formatUnits(order.filled), size: formatUnits(order.size), avg: formatUsdt(order.avg_fill_price), reason: order.reject_reason ?? "" };
    return (
      <div className="px-3 pb-3">
        <Alert variant={order.state === "rejected" ? "destructive" : undefined}>
          {resting ? <Clock className="size-4" /> : <CheckCircle2 className="size-4" />}
          <AlertTitle>{key ? t(key) : (order.state ?? "")}</AlertTitle>
          <AlertDescription>{t(`trade.form.placed.${order.state === "rejected" ? "rejected" : key === "trade.state.partialCancelled" ? "partial" : resting ? "resting" : order.state === "filled" ? "filled" : "cancelled"}`, args)}</AlertDescription>
        </Alert>
      </div>
    );
  }
  // 412 is the hub's "not for you": book closed, access below `invest`, or a market order
  // into an empty side. 409 is the retry key doing its job — the order already landed.
  const status = outcome.error instanceof RequestError ? outcome.error.status : 0;
  const title = status === 412 ? t("trade.form.refusedTitle") : status === 409 ? t("trade.form.duplicateTitle") : t("trade.form.failedTitle");
  return (
    <div className="px-3 pb-3">
      <Alert variant="destructive">
        <TriangleAlert className="size-4" />
        <AlertTitle>{title}</AlertTitle>
        <AlertDescription>{status === 412 ? t("trade.form.refusedBody", { detail: errorMessage(outcome.error, t) }) : errorMessage(outcome.error, t)}</AlertDescription>
      </Alert>
    </div>
  );
}
