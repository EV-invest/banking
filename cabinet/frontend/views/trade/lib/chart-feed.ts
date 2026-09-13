"use client";

// Where the chart's bars come from, behind one interface.
//
// The plotting engine today is lightweight-charts; TradingView's Advanced Charts takes a
// `Datafeed` with the same two verbs — a history request and a subscription — so the
// terminal is written against this seam rather than against either library. Swapping the
// engine later means a new `use-candle-chart`, not a new feed.
//
// History comes over REST (`/api/book/candles`). The live leg is derived from the public
// tape the socket already delivers: the newest trade on each frame is folded into the
// open bar (`applyTrade`), so no second stream and no per-tick `setData`.

import { fetchCandles } from "@/entities/book/api/book-client";
import { bookTradesResource } from "@/entities/book/model/book-resource";
import type { CandleResolution, Trade } from "@/shared/contracts/book";
import { applyTrade, barFromCandle, historyFrom, type Bar } from "@/views/trade/lib/candles";

export interface ChartFeed {
  /** Every bar from `from` to `to` (unix seconds, `to` absent = now), oldest first. */
  history(resolution: CandleResolution, from: number, to?: number): Promise<Bar[]>;
  /** Live bars for one resolution. `onBar` receives the open bar after each trade; the
   *  same `time` means "replace", a later one means "append". Returns the unsubscribe. */
  subscribe(resolution: CandleResolution, last: Bar | null, onBar: (bar: Bar) => void): () => void;
}

/** The feed over the BFF's REST history and the socket-fed tape, for one product. */
export function restChartFeed(service: string): ChartFeed {
  return {
    async history(resolution, from, to) {
      const list = await fetchCandles(service, resolution, from, to);
      return (list.candles ?? []).map(barFromCandle).filter((bar): bar is Bar => bar !== null);
    },

    subscribe(resolution, last, onBar) {
      let bar = last;
      // The tape is newest-first, and a frame may carry several prints at once. Fold in
      // every trade newer than the last one seen, oldest first, so a burst of fills lands
      // as one correct bar rather than as the newest print alone.
      let seen = newestId(bookTradesResource.peek(service)?.trades);
      return bookTradesResource.watch((snapshot) => {
        const fresh = takeNewer(snapshot.data?.trades ?? [], seen);
        if (fresh.length === 0) return;
        seen = fresh[fresh.length - 1]?.id ?? seen;
        for (const trade of fresh) {
          const next = applyTrade(bar, trade, resolution);
          if (next) {
            bar = next;
            onBar(next);
          }
        }
      }, service);
    },
  };
}

/** How far back the first history request reaches for a resolution. */
export function historyWindow(resolution: CandleResolution, now = Math.floor(Date.now() / 1000)): { from: number; to: number } {
  return { from: historyFrom(now, resolution), to: now };
}

const newestId = (trades: readonly Trade[] | undefined): string | undefined => trades?.[0]?.id;

/** The prints newer than `seen`, oldest first. Unknown `seen` (first subscribe) yields none —
 *  history already covers what is on the tape. */
function takeNewer(trades: readonly Trade[], seen: string | undefined): Trade[] {
  if (seen === undefined) return [];
  const fresh: Trade[] = [];
  for (const trade of trades) {
    if (trade.id === seen) break;
    fresh.push(trade);
  }
  return fresh.reverse();
}
