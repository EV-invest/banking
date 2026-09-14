"use client";

// `/invest/[service]/trade` — the terminal over one product's book.
//
// Laid out like a spot exchange (ticker, then chart | book | order form, then the
// trader's own orders) on the uikit's `Terminal` frame; each pane is its own file and
// reads its own resources, so this view only resolves WHICH product and wires the one
// piece of state two panes share — the price a click on a book level hands to the form.
//
// The product is resolved the way the product page resolves it: from the catalog and the
// positions, both cached. A slug that is not a registered allocation renders the same
// not-found state — the hub refuses it too, so a wrong link can never become a book.

import { TriangleAlert } from "lucide-react";
import { useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle, Skeleton, Terminal } from "@evinvest/uikit";

import { bookPolicyResource } from "@/entities/book/model/book-resource";
import { useBookStream } from "@/entities/book/model/book-socket";
import { allocationsResource, positionsResource } from "@/entities/fund/model/fund-resource";
import { useResource } from "@/shared/lib/resource";
import { buildProducts, type Product } from "@/views/invest/lib/product";
import { BookPane } from "@/views/trade/ui/book-pane";
import { ChartPane } from "@/views/trade/ui/chart-pane";
import { OrderFormPane, type PricePick } from "@/views/trade/ui/order-form-pane";
import { OrdersPane } from "@/views/trade/ui/orders-pane";
import { TickerPane } from "@/views/trade/ui/ticker-pane";

export function TradeView({ service }: { service: string }) {
  const t = useT();
  const catalogList = useResource(allocationsResource);
  const positionList = useResource(positionsResource);
  const policyRead = useResource(bookPolicyResource, service);
  const [pick, setPick] = useState<PricePick | null>(null);

  const resolving = catalogList.isLoading || positionList.isLoading;
  // `undefined` is still "loading", `null` is "no such product" — collapsing the two would
  // flash the not-found state on every cold load.
  const product: Product | null | undefined = resolving
    ? undefined
    : (buildProducts(catalogList.data?.allocations ?? [], positionList.data?.positions ?? []).find((p) => p.service === service) ?? null);

  // The feed is opened only for a product that exists; hooks run unconditionally, the
  // subscription does not.
  const stream = useBookStream(service, product !== undefined && product !== null);

  if (product === undefined) {
    return (
      <div className="container space-y-4 py-8">
        <Skeleton className="h-12 w-full" />
        <Skeleton className="h-96 w-full" />
      </div>
    );
  }

  if (product === null) {
    return (
      <div className="container max-w-4xl py-12">
        <Empty className="rounded-xl border border-border bg-card p-6 md:p-8">
          <EmptyHeader>
            <EmptyMedia variant="icon">
              <TriangleAlert className="size-5" />
            </EmptyMedia>
            <EmptyTitle>{t("invest.notRegistered", { service })}</EmptyTitle>
            <EmptyDescription>{t("invest.notRegisteredHint")}</EmptyDescription>
          </EmptyHeader>
        </Empty>
      </div>
    );
  }

  // A delisted product (absent from the catalog, still held) and one the caller may only
  // view are both locked out of the form; the book itself stays readable, since a price
  // is public information about a product the caller can see.
  const locked = product.allocation === null || product.allocation.caller_access === "view";

  return (
    <Terminal className="border-t border-border">
      <TickerPane product={product} status={stream.status} />
      <ChartPane service={service} />
      <BookPane service={service} onPick={(price) => setPick({ price, at: Date.now() })} />
      <OrderFormPane service={service} policy={policyRead.data ?? null} policyKnown={!policyRead.isLoading} position={product.position} locked={locked} pick={pick} />
      <OrdersPane service={service} />
    </Terminal>
  );
}
