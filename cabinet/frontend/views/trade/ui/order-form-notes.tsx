"use client";

// What the form says around its controls: the gate that closes it, and the outcome of
// the last submit. Both stay on screen beside the form — the cabinet mounts no toaster,
// and an answer that names money that moved must not slide away.

import { CheckCircle2, Clock, Info, Lock, TriangleAlert } from "lucide-react";

import { useLocale, useT } from "@evinvest/i18n/react";
import { Alert, AlertDescription, AlertTitle } from "@evinvest/uikit";

import type { Order } from "@/shared/contracts/book";
import { RequestError, errorMessage } from "@/shared/lib/api-client";
import { SupportLink } from "@/shared/ui/support-link";
import { Note } from "@/views/invest/ui/atoms";
import { formatUnits, formatUsdt } from "@/views/trade/lib/format";
import { orderStateKey, placedOutcome } from "@/views/trade/lib/order-state";

export type Outcome = { order: Order; error?: never } | { error: unknown; order?: never };

/** Why the form is closed, stated before the action — or, on an open book whose units the
 *  fund holds no cash for, what a buyer is actually buying. One line either way: a closed
 *  book already says there is nothing to buy, so the exit warning waits until it opens.
 *  A lock is the reader's access, not the book's state, so it carries the way to ask. */
export function OrderGate({ closed, locked, unbacked }: { closed: boolean; locked: boolean; unbacked: boolean }) {
  const t = useT();
  if (closed || locked) {
    return (
      <div className="px-3 pt-3">
        <Note tone={closed ? "muted" : "amber"}>
          <Lock className="mr-1.5 inline size-3.5 align-text-bottom" />
          {t(closed ? "trade.form.closed" : "trade.form.locked")}
          {!closed && (
            <>
              {" "}
              <SupportLink />
            </>
          )}
        </Note>
      </div>
    );
  }
  if (!unbacked) return null;
  return (
    <div className="px-3 pt-3">
      <Note tone="amber">
        <Info className="mr-1.5 inline size-3.5 align-text-bottom" />
        {t("trade.unbackedNotice")}
      </Note>
    </div>
  );
}

/** The last submit's answer: the order as the hub recorded it, or why it refused. */
export function OrderOutcome({ outcome }: { outcome: Outcome }) {
  const t = useT();
  const locale = useLocale();
  if (outcome.order) {
    const order = outcome.order;
    const key = orderStateKey(order);
    const placed = placedOutcome(order);
    const args = { filled: formatUnits(order.filled, locale), size: formatUnits(order.size, locale), avg: formatUsdt(order.avg_fill_price, locale), reason: order.reject_reason ?? "" };
    return (
      <div className="px-3 pb-3">
        <Alert variant={placed === "rejected" ? "destructive" : undefined}>
          {placed === "resting" ? <Clock className="size-4" /> : <CheckCircle2 className="size-4" />}
          <AlertTitle>{key ? t(key) : (order.state ?? "")}</AlertTitle>
          <AlertDescription>{t(`trade.form.placed.${placed}`, args)}</AlertDescription>
        </Alert>
      </div>
    );
  }
  // 412 is the hub's "not for you": book closed, access below `invest`, or a market order
  // into an empty side. 409 is the retry key doing its job — the order already landed.
  const status = outcome.error instanceof RequestError ? outcome.error.status : 0;
  const refused = status === 412;
  const title = refused ? t("trade.form.refusedTitle") : status === 409 ? t("trade.form.duplicateTitle") : t("trade.form.failedTitle");
  return (
    <div className="px-3 pb-3">
      <Alert variant="destructive">
        <TriangleAlert className="size-4" />
        <AlertTitle>{title}</AlertTitle>
        <AlertDescription>
          {refused ? t("trade.form.refusedBody", { detail: errorMessage(outcome.error, t) }) : errorMessage(outcome.error, t)}
          {refused && (
            <>
              {" "}
              <SupportLink />
            </>
          )}
        </AlertDescription>
      </Alert>
    </div>
  );
}
