"use client";

// What the form says around its controls: the gate that closes it, and the outcome of
// the last submit. Both stay on screen beside the form — the cabinet mounts no toaster,
// and an answer that names money that moved must not slide away.

import { CheckCircle2, Clock, Info, Lock, TriangleAlert } from "lucide-react";

import type { MessageValues, Translate } from "@evinvest/i18n";
import { useLocale, useT } from "@evinvest/i18n/react";
import { Alert, AlertDescription, AlertTitle } from "@evinvest/uikit";

import type { Order } from "@/shared/contracts/book";
import { RequestError, errorMessage } from "@/shared/lib/api-client";
import { SupportLink } from "@/shared/ui/support-link";
import { Note } from "@/views/invest/ui/atoms";
import { formatUnits, formatUsdt } from "@/views/trade/lib/format";
import { orderStateLabel, type PlacedOutcome, placedOutcome } from "@/views/trade/lib/order-state";

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
          {(closed ? t("trade.form.closed", "The book is closed. Orders are not accepted until it opens again.") : t("trade.form.locked", "Trading isn't open to you on this product. If you think you should have access:"))}
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
        {t("trade.unbackedNotice", "Units of this product are a claim on an off-platform asset and cannot be redeemed for fund cash — the book is the only exit.")}
      </Note>
    </div>
  );
}

/** The last submit's answer: the order as the hub recorded it, or why it refused. */
const placedWords = (t: Translate, args: MessageValues): Record<PlacedOutcome, string> => ({
  rejected: t("trade.form.placed.rejected", "The order was not accepted: {reason}", args),
  partial: t("trade.form.placed.partial", "Filled {filled} of {size} at {avg} USDT; the rest was cancelled.", args),
  resting: t("trade.form.placed.resting", "Resting on the book: {filled} of {size} filled so far.", args),
  filled: t("trade.form.placed.filled", "Filled {filled} at an average of {avg} USDT.", args),
  cancelled: t("trade.form.placed.cancelled", "Nothing crossed, so the order was cancelled.", args),
});

export function OrderOutcome({ outcome }: { outcome: Outcome }) {
  const t = useT();
  const locale = useLocale();
  if (outcome.order) {
    const order = outcome.order;
    const stateLabel = orderStateLabel(order, t);
    const placed = placedOutcome(order);
    const args = { filled: formatUnits(order.filled, locale), size: formatUnits(order.size, locale), avg: formatUsdt(order.avg_fill_price, locale), reason: order.reject_reason ?? "" };
    return (
      <div className="px-3 pb-3">
        <Alert variant={placed === "rejected" ? "destructive" : undefined}>
          {placed === "resting" ? <Clock className="size-4" /> : <CheckCircle2 className="size-4" />}
          <AlertTitle>{stateLabel ?? (order.state ?? "")}</AlertTitle>
          <AlertDescription>{placedWords(t, args)[placed]}</AlertDescription>
        </Alert>
      </div>
    );
  }
  // 412 is the hub's "not for you": book closed, access below `invest`, or a market order
  // into an empty side. 409 is the retry key doing its job — the order already landed.
  const status = outcome.error instanceof RequestError ? outcome.error.status : 0;
  const refused = status === 412;
  const title = refused ? t("trade.form.refusedTitle", "Order refused") : status === 409 ? t("trade.form.duplicateTitle", "Already submitted") : t("trade.form.failedTitle", "Order failed");
  return (
    <div className="px-3 pb-3">
      <Alert variant="destructive">
        <TriangleAlert className="size-4" />
        <AlertTitle>{title}</AlertTitle>
        <AlertDescription>
          {refused ? t("trade.form.refusedBody", "{detail} If you think you should be able to trade this product:", { detail: errorMessage(outcome.error, t) }) : errorMessage(outcome.error, t)}
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
