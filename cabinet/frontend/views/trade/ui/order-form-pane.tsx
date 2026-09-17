"use client";

// The order form: side, kind, time in force, price, size — and, before the submit, what
// the order costs and what is there to pay it with. The arithmetic and the wire body live
// in `../lib/order-form`; this file owns the state, the submit and the outcome.
//
// A closed book and a caller locked below `invest` are stated before the action rather
// than after a failed submit; a 412 that still comes back (the gate moved under us) is
// shown the same way, as "contact support" with the address, rather than as an error code.

import { useRef, useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Tabs, TabsList, TabsTrigger, TerminalPane, TerminalPaneBody, TerminalPaneHeader } from "@evinvest/uikit";

import { bookSnapshotResource, placeOrder } from "@/entities/book/model/book-resource";
import { walletResource } from "@/entities/wallet/model/wallet-resource";
import type { Position } from "@/shared/contracts";
import type { BookPolicy, BookSnapshot, Order, OrderSide } from "@/shared/contracts/book";
import { useResource } from "@/shared/lib/resource";
import { TipAnchor } from "@/shared/tips";
import { EMPTY_ORDER_DRAFT, orderSubmissionFor, placeOrderBody, type OrderContext, type OrderDraft, type OrderSubmission } from "@/views/trade/lib/order-form";
import { OrderFormFields } from "@/views/trade/ui/order-form-fields";
import { OrderGate, OrderOutcome, type Outcome } from "@/views/trade/ui/order-form-notes";

/** A price handed over by a click on a book level. `at` makes two clicks on the same
 *  level two picks — the trader may have edited the price in between. */
export interface PricePick {
  price: string;
  at: number;
}

/** What a market order would trade around: the best level on the other side, else the
 *  last print. `null` on an empty book, which is exactly when a market order is refused. */
function referencePrice(book: BookSnapshot | null, side: OrderSide): string | null {
  const opposite = side === "buy" ? book?.asks?.[0]?.price : book?.bids?.[0]?.price;
  return opposite || book?.last_price || null;
}

export function OrderFormPane({
  service,
  policy,
  policyKnown,
  position,
  locked,
  pick,
}: {
  service: string;
  policy: BookPolicy | null;
  /** The policy read has settled — a form must not say "closed" while it is still loading. */
  policyKnown: boolean;
  position: Position | null;
  locked: boolean;
  pick: PricePick | null;
}) {
  const t = useT();
  const wallet = useResource(walletResource);
  const book = useResource(bookSnapshotResource, service);
  const [draft, setDraft] = useState<OrderDraft>(EMPTY_ORDER_DRAFT);
  const [busy, setBusy] = useState(false);
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  // Read and written only inside the submit handler: the id must survive a failed
  // attempt without triggering a render, which is what a ref is for.
  const submission = useRef<OrderSubmission | null>(null);

  // A pick is an event, applied during render the way `Settled` (shared/ui/motion) adjusts state
  // — never in an effect, which would paint one frame with the old price.
  const [seenPick, setSeenPick] = useState(pick);
  if (pick !== seenPick) {
    setSeenPick(pick);
    if (pick) setDraft((d) => ({ ...d, kind: "limit", price: pick.price }));
  }

  const context: OrderContext = {
    policy,
    availableCash: wallet.data?.balance?.available ?? null,
    availableUnits: position?.units ?? (position === null ? "0" : null),
    referencePrice: referencePrice(book.data ?? null, draft.side),
  };
  const closed = policyKnown && !policy?.book_open;
  const disabled = busy || closed || locked || !policyKnown;

  const submit = async () => {
    const key = orderSubmissionFor(submission.current, service, draft);
    submission.current = key;
    const body = placeOrderBody(service, draft, context, key.id);
    if (!body) return;
    setBusy(true);
    setOutcome(null);
    try {
      const order: Order = await placeOrder(body);
      setOutcome({ order });
      // The price stays — the next order is usually near the last — the size does not.
      setDraft((d) => ({ ...d, size: "" }));
      submission.current = null;
    } catch (error) {
      setOutcome({ error });
    } finally {
      setBusy(false);
    }
  };

  return (
    <TerminalPane area="form">
      <TerminalPaneHeader>
        <Tabs value={draft.side} onValueChange={(v) => setDraft((d) => ({ ...d, side: v === "sell" ? "sell" : "buy" }))} className="w-full">
          <TabsList className="h-7 w-full">
            <TabsTrigger value="buy" className="text-xs data-[state=active]:text-positive">
              {t("trade.form.buy")}
            </TabsTrigger>
            <TabsTrigger value="sell" className="text-xs data-[state=active]:text-accent-error">
              {t("trade.form.sell")}
            </TabsTrigger>
          </TabsList>
        </Tabs>
      </TerminalPaneHeader>
      <TerminalPaneBody>
        {/* Collapsed by default (see the catalog): the book vs. NAV subscription is the one
            thing a newcomer to this screen has not been told, and the form is where they
            find out they need it — but the form's controls must not move to make room. */}
        <TipAnchor anchor="trade.book" className="mx-3 mt-3" />
        <OrderGate closed={closed} locked={locked} unbacked={policy?.allow_unbacked_trading === true} />
        <OrderFormFields draft={draft} context={context} position={position} disabled={disabled} busy={busy} onChange={setDraft} onSubmit={submit} />
        {outcome && <OrderOutcome outcome={outcome} />}
      </TerminalPaneBody>
    </TerminalPane>
  );
}
