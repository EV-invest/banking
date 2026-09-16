"use client";

// The book itself: asks above the spread line, bids below, each level's bar sized by its
// cumulative depth. A click (or Enter) on a level hands its price to the form — the
// fastest way to quote is to join or improve a level that is already there.

import type { KeyboardEvent } from "react";

import { useLocale, useT } from "@evinvest/i18n/react";
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle, OrderBook, OrderBookHead, OrderBookRow, OrderBookSpread, Skeleton } from "@evinvest/uikit";

import { bookSnapshotResource } from "@/entities/book/model/book-resource";
import { useResource } from "@/shared/lib/resource";
import { ResourceError } from "@/shared/ui/resource-error";
import { depthRows, type DepthRow } from "@/views/trade/lib/depth";
import { formatUnits, formatUsdt } from "@/views/trade/lib/format";

const ROW_FOCUS = "outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring";

export function BookLevels({ service, onPick }: { service: string; onPick: (price: string) => void }) {
  const t = useT();
  const locale = useLocale();
  const read = useResource(bookSnapshotResource, service);
  const book = read.data ?? null;

  if (!book) {
    if (read.error) return <ResourceError error={read.error} className="p-3" />;
    return (
      <div className="space-y-2 p-3">
        <Skeleton className="h-4 w-full" />
        <Skeleton className="h-4 w-full" />
        <Skeleton className="h-4 w-2/3" />
      </div>
    );
  }

  const { bids, asks } = depthRows(book.bids, book.asks);
  if (bids.length === 0 && asks.length === 0) {
    return (
      <Empty className="m-3 border border-dashed border-border p-4">
        <EmptyHeader>
          <EmptyTitle className="text-sm">{t("trade.book.empty")}</EmptyTitle>
          <EmptyDescription className="text-xs">{t("trade.book.emptyHint")}</EmptyDescription>
        </EmptyHeader>
      </Empty>
    );
  }

  const level = (side: "bid" | "ask", row: DepthRow) => {
    const pick = () => onPick(row.price);
    const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        pick();
      }
    };
    return (
      <OrderBookRow
        key={`${side}:${row.price}`}
        side={side}
        price={formatUsdt(row.price, locale)}
        size={formatUnits(row.size, locale)}
        total={formatUnits(row.total, locale)}
        depth={row.depth}
        role="button"
        tabIndex={0}
        title={t("trade.book.levelOrders", { n: row.orders })}
        className={ROW_FOCUS}
        onClick={pick}
        onKeyDown={onKeyDown}
      />
    );
  };

  return (
    <OrderBook>
      <OrderBookHead price={t("trade.book.col.price")} size={t("trade.book.col.size")} total={t("trade.book.col.total")} />
      {/* Asks arrive best (lowest) first; drawn top-down they must end at the spread, so
          the order is reversed for display and the best ask sits just above the line. */}
      {[...asks].reverse().map((row) => level("ask", row))}
      <OrderBookSpread>
        <span>{book.mid ? formatUsdt(book.mid, locale) : "—"}</span>
        <span className="text-xs">{book.spread ? t("trade.book.spread", { spread: formatUsdt(book.spread, locale) }) : t("trade.book.oneSided")}</span>
      </OrderBookSpread>
      {bids.map((row) => level("bid", row))}
    </OrderBook>
  );
}
