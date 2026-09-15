"use client";

// One table for both the resting orders and the history: the columns are the same, only
// the cancel control differs, and it is offered exactly when an order can still be
// cancelled — never on a filled, cancelled or rejected row.

import { Loader2, X } from "lucide-react";

import { useT } from "@evinvest/i18n/react";
import { Button, Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@evinvest/uikit";

import type { Order } from "@/shared/contracts/book";
import { useLocale } from "@/shared/lib/cabinet-route";
import { cn } from "@/shared/lib/cn";
import { formatUnits, formatUsdt, formatWhen } from "@/views/trade/lib/format";
import { isResting, orderStateKey } from "@/views/trade/lib/order-state";

const HEAD = "h-8 text-xs font-medium text-ink-soft";
const CELL = "py-1.5 font-mono-tech text-xs tabular-nums";

export function OrdersTable({ orders, busyId, onCancel }: { orders: Order[]; busyId?: string | null; onCancel?: (orderId: string) => void }) {
  const t = useT();
  const locale = useLocale();
  return (
    <Table className="text-xs">
      <TableHeader>
        <TableRow>
          <TableHead className={HEAD}>{t("trade.orders.col.time")}</TableHead>
          <TableHead className={HEAD}>{t("trade.orders.col.side")}</TableHead>
          <TableHead className={HEAD}>{t("trade.orders.col.type")}</TableHead>
          <TableHead className={cn(HEAD, "text-right")}>{t("trade.book.col.price")}</TableHead>
          <TableHead className={cn(HEAD, "text-right")}>{t("trade.book.col.size")}</TableHead>
          <TableHead className={cn(HEAD, "text-right")}>{t("trade.orders.col.filled")}</TableHead>
          <TableHead className={cn(HEAD, "text-right")}>{t("trade.orders.col.avgFill")}</TableHead>
          <TableHead className={cn(HEAD, "text-right")}>{t("trade.orders.col.fee")}</TableHead>
          <TableHead className={HEAD}>{t("trade.orders.col.state")}</TableHead>
          {onCancel && <TableHead className={HEAD} />}
        </TableRow>
      </TableHeader>
      <TableBody>
        {orders.map((order) => {
          const id = order.id ?? "";
          const stateKey = orderStateKey(order);
          return (
            <TableRow key={id}>
              <TableCell className={cn(CELL, "text-ink-soft")}>{formatWhen(order.created_at, locale)}</TableCell>
              <TableCell className={cn(CELL, "font-semibold", order.side === "sell" ? "text-accent-error" : "text-positive")}>{t(order.side === "sell" ? "trade.form.sell" : "trade.form.buy")}</TableCell>
              <TableCell className={CELL}>
                {t(order.kind === "market" ? "trade.form.market" : "trade.form.limit")}
                {order.kind !== "market" && order.tif && <span className="ml-1 uppercase text-ink-soft">{order.tif}</span>}
              </TableCell>
              <TableCell className={cn(CELL, "text-right")}>{formatUsdt(order.price)}</TableCell>
              <TableCell className={cn(CELL, "text-right")}>{formatUnits(order.size)}</TableCell>
              <TableCell className={cn(CELL, "text-right")}>{formatUnits(order.filled)}</TableCell>
              <TableCell className={cn(CELL, "text-right")}>{order.avg_fill_price ? formatUsdt(order.avg_fill_price) : "—"}</TableCell>
              <TableCell className={cn(CELL, "text-right")}>{formatUsdt(order.fee_paid)}</TableCell>
              {/* An unmapped state falls back to the wire word — a value the hub added that
                  this build has no word for, shown rather than swallowed. */}
              <TableCell className={cn(CELL, order.state === "rejected" && "text-accent-error")} title={order.reject_reason || undefined}>
                {stateKey ? t(stateKey) : (order.state ?? "—")}
              </TableCell>
              {onCancel && (
                <TableCell className={cn(CELL, "text-right")}>
                  {isResting(order) && (
                    <Button type="button" variant="outline" size="sm" className="h-6 px-2 text-xs" disabled={busyId === id} onClick={() => onCancel(id)}>
                      {busyId === id ? <Loader2 className="size-3 animate-spin" /> : <X className="size-3" />}
                      {t("ui.cancel")}
                    </Button>
                  )}
                </TableCell>
              )}
            </TableRow>
          );
        })}
      </TableBody>
    </Table>
  );
}
