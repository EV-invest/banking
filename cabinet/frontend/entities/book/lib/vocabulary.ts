// The book's closed vocabularies, as the wire spells them — mirrored from
// `evbanking_contracts::book`, which the BFF checks before the hub is called. A value
// outside these lists is refused with a 400 naming the words that would have worked, so
// the lists here and there have to be edited together.
//
// Import-free, so `node --test` can exercise the modules built on it.

import type { CandleResolution, OrderKind, OrderSide, OrderTif } from "@/shared/contracts/book";

export const ORDER_SIDES: readonly OrderSide[] = ["buy", "sell"];
export const ORDER_KINDS: readonly OrderKind[] = ["limit", "market"];
export const ORDER_TIFS: readonly OrderTif[] = ["gtc", "ioc", "alo"];
export const RESOLUTIONS: readonly CandleResolution[] = ["1m", "5m", "15m", "1h", "4h", "1d"];

/** Seconds per candle bucket — what the chart needs to extend the last bar from trades. */
export const RESOLUTION_SECONDS: Record<CandleResolution, number> = {
  "1m": 60,
  "5m": 300,
  "15m": 900,
  "1h": 3_600,
  "4h": 14_400,
  "1d": 86_400,
};

export const isOrderSide = (value: unknown): value is OrderSide => ORDER_SIDES.some((s) => s === value);
export const isResolution = (value: unknown): value is CandleResolution => RESOLUTIONS.some((r) => r === value);

/** The other side of a trade or a book level — a bid is lifted by a `sell`, an ask by a `buy`. */
export const opposite = (side: OrderSide): OrderSide => (side === "buy" ? "sell" : "buy");
