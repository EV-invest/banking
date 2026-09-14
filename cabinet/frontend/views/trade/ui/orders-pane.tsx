"use client";

// The trader's own orders, under the terminal: what is resting (and can be cancelled),
// what happened to everything else, and the fills behind it. Three REST reads, each
// refreshed when the socket says `orders_revision` moved.

import { useState, type ReactNode } from "react";

import { useT } from "@evinvest/i18n/react";
import { OpenOrdersEmpty, Skeleton, Tabs, TabsList, TabsTrigger, TerminalPane, TerminalPaneBody, TerminalPaneHeader } from "@evinvest/uikit";

import { cancelOrder, fillsResource, openOrdersResource, orderHistoryResource } from "@/entities/book/model/book-resource";
import { errorMessage } from "@/shared/lib/api-client";
import { useResource, type Resource, type ResourceSnapshot } from "@/shared/lib/resource";
import { ResourceError } from "@/shared/ui/resource-error";
import { FillsTable } from "@/views/trade/ui/fills-table";
import { OrdersTable } from "@/views/trade/ui/orders-table";

type OrdersTab = "open" | "history" | "fills";
const TABS: readonly OrdersTab[] = ["open", "history", "fills"];

export function OrdersPane({ service }: { service: string }) {
  const t = useT();
  const [tab, setTab] = useState<OrdersTab>("open");
  const [busy, setBusy] = useState<string | null>(null);
  const [actionError, setActionError] = useState<unknown>(null);

  const cancel = async (orderId: string) => {
    setBusy(orderId);
    setActionError(null);
    try {
      await cancelOrder(orderId);
    } catch (e) {
      setActionError(e);
    } finally {
      setBusy(null);
    }
  };

  return (
    <TerminalPane area="orders">
      <TerminalPaneHeader>
        <Tabs value={tab} onValueChange={(v) => setTab(TABS.find((x) => x === v) ?? "open")}>
          <TabsList className="h-7">
            {TABS.map((key) => (
              <TabsTrigger key={key} value={key} className="text-xs">
                {t(`trade.orders.${key}`)}
              </TabsTrigger>
            ))}
          </TabsList>
        </Tabs>
      </TerminalPaneHeader>
      <TerminalPaneBody>
        {actionError !== null && <ResourceError message={errorMessage(actionError, t)} className="px-3 pt-2" />}
        {tab === "open" && <OrdersRead resource={openOrdersResource} service={service} select={(d) => d.orders} empty={t("trade.orders.emptyOpen")} render={(orders) => <OrdersTable orders={orders} busyId={busy} onCancel={cancel} />} />}
        {tab === "history" && <OrdersRead resource={orderHistoryResource} service={service} select={(d) => d.orders} empty={t("trade.orders.emptyHistory")} render={(orders) => <OrdersTable orders={orders} />} />}
        {tab === "fills" && <OrdersRead resource={fillsResource} service={service} select={(d) => d.trades} empty={t("trade.orders.emptyFills")} render={(trades) => <FillsTable trades={trades} />} />}
      </TerminalPaneBody>
    </TerminalPane>
  );
}

/** One tab's read, with its loading, failed and empty states drawn the same way. */
function OrdersRead<T, R>({
  resource,
  service,
  select,
  empty,
  render,
}: {
  resource: Resource<T, [service: string]>;
  service: string;
  /** The list inside the wire object — proto wraps every list in a message. */
  select: (data: T) => R[] | undefined;
  empty: string;
  render: (rows: R[]) => ReactNode;
}) {
  const read: ResourceSnapshot<T> = useResource(resource, service);
  if (read.data === undefined) {
    if (read.error) return <ResourceError error={read.error} className="p-3" />;
    return (
      <div className="space-y-2 p-3">
        <Skeleton className="h-5 w-full" />
        <Skeleton className="h-5 w-3/4" />
      </div>
    );
  }
  const rows = select(read.data) ?? [];
  if (rows.length === 0) return <OpenOrdersEmpty>{empty}</OpenOrdersEmpty>;
  return render(rows);
}
