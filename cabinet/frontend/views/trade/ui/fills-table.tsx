"use client";

// The caller's own fills: their side, the price and size, and the fee they paid — empty
// when they were the maker. Never the counterparty; the wire does not carry one.

import { useT } from "@evinvest/i18n/react";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@evinvest/uikit";

import type { Trade } from "@/shared/contracts/book";
import { useLocale } from "@/shared/lib/cabinet-route";
import { cn } from "@/shared/lib/cn";
import { formatUnits, formatUsdt, formatWhen } from "@/views/trade/lib/format";

const HEAD = "h-8 text-xs font-medium text-ink-soft";
const CELL = "py-1.5 font-mono-tech text-xs tabular-nums";

export function FillsTable({ trades }: { trades: Trade[] }) {
  const t = useT();
  const locale = useLocale();
  return (
    <Table className="text-xs">
      <TableHeader>
        <TableRow>
          <TableHead className={HEAD}>{t("trade.orders.col.time")}</TableHead>
          <TableHead className={HEAD}>{t("trade.orders.col.side")}</TableHead>
          <TableHead className={cn(HEAD, "text-right")}>{t("trade.book.col.price")}</TableHead>
          <TableHead className={cn(HEAD, "text-right")}>{t("trade.book.col.size")}</TableHead>
          <TableHead className={cn(HEAD, "text-right")}>{t("trade.orders.col.fee")}</TableHead>
          <TableHead className={HEAD}>{t("trade.orders.col.role")}</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {trades.map((trade, i) => {
          const sell = trade.user_side === "sell";
          // The caller was the taker exactly when their side is the side that crossed.
          const taker = trade.user_side !== "" && trade.user_side === trade.taker_side;
          return (
            <TableRow key={trade.id ?? i}>
              <TableCell className={cn(CELL, "text-ink-soft")}>{formatWhen(trade.executed_at, locale)}</TableCell>
              <TableCell className={cn(CELL, "font-semibold", sell ? "text-accent-error" : "text-positive")}>{t(sell ? "trade.form.sell" : "trade.form.buy")}</TableCell>
              <TableCell className={cn(CELL, "text-right")}>{formatUsdt(trade.price, locale)}</TableCell>
              <TableCell className={cn(CELL, "text-right")}>{formatUnits(trade.size, locale)}</TableCell>
              <TableCell className={cn(CELL, "text-right")}>{trade.fee ? formatUsdt(trade.fee, locale) : "—"}</TableCell>
              <TableCell className={cn(CELL, "text-ink-soft")}>{t(taker ? "trade.orders.taker" : "trade.orders.maker")}</TableCell>
            </TableRow>
          );
        })}
      </TableBody>
    </Table>
  );
}
