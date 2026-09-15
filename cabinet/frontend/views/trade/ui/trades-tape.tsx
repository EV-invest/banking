"use client";

// The public tape: every print, newest first, coloured by the side that crossed the
// spread. No parties — the hub never discloses them, and the wire has no field for it.

import { useT } from "@evinvest/i18n/react";
import { OpenOrdersEmpty, Skeleton, TradesTapeRow } from "@evinvest/uikit";

import { bookTradesResource } from "@/entities/book/model/book-resource";
import { useLocale } from "@/shared/lib/cabinet-route";
import { useResource } from "@/shared/lib/resource";
import { ResourceError } from "@/shared/ui/resource-error";
import { formatClock, formatUnits, formatUsdt } from "@/views/trade/lib/format";

export function TradesTape({ service }: { service: string }) {
  const t = useT();
  const locale = useLocale();
  const read = useResource(bookTradesResource, service);
  const trades = read.data?.trades ?? null;

  if (!trades) {
    if (read.error) return <ResourceError error={read.error} className="p-3" />;
    return (
      <div className="space-y-2 p-3">
        <Skeleton className="h-4 w-full" />
        <Skeleton className="h-4 w-full" />
      </div>
    );
  }
  if (trades.length === 0) return <OpenOrdersEmpty>{t("trade.tape.empty")}</OpenOrdersEmpty>;

  return (
    <div className="py-1">
      <div className="grid grid-cols-3 gap-2 px-3 py-1 text-right text-xs text-ink-soft [&>*:first-child]:text-left">
        <span>{t("trade.book.col.price")}</span>
        <span>{t("trade.book.col.size")}</span>
        <span>{t("trade.tape.col.time")}</span>
      </div>
      {trades.map((trade, i) => (
        // A taker buy lifted an ask and prints in the bid colour — the buyer's side.
        <TradesTapeRow key={trade.id ?? i} side={trade.taker_side === "sell" ? "ask" : "bid"} price={formatUsdt(trade.price)} size={formatUnits(trade.size)} time={formatClock(trade.executed_at, locale)} />
      ))}
    </div>
  );
}
