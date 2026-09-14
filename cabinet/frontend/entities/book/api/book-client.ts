// Browser → BFF book client. Thin typed fetchers over the `/api/book/*` routes (see
// `cabinet/backend/README.md` § The book surface); the shapes are the proto-derived types
// from `@/shared/contracts/book`. Transport, CSRF and session handling belong to
// `@/shared/lib/api-client`. No tokens are ever seen here — the BFF holds them.
//
// The fail-fast guard rejects with a keyed `RequestError` for the same reason
// `entities/fund/api/fund-client.ts` does: the rejection ends up on screen through
// `ResourceError`, and only a keyed error reads in the reader's language.

import { getJson, postJson, RequestError } from "@/shared/lib/api-client";
import { bookPath } from "@/entities/book/lib/wire";
import type { BookPolicy, BookSnapshot, CandleList, CandleResolution, Order, OrderList, PlaceOrderBody, TradeList } from "@/shared/contracts/book";

function withService<T>(service: string, read: () => Promise<T>): Promise<T> {
  // Never issue a bare `/api/book?service=` — the BFF 400s without one, so fail fast here.
  if (!service.trim()) return Promise.reject(new RequestError("fund service required", 400, "err.fundServiceRequired"));
  return read();
}

/** The book as of now: aggregated levels, the last trade, mid/spread, NAV, 24h figures. */
export function fetchBook(service: string, depth?: number): Promise<BookSnapshot> {
  return withService(service, () => getJson<BookSnapshot>(bookPath("/api/book", { service, depth })));
}

/** The public tape — prices, sizes and the taker's side; never who traded. */
export function fetchTrades(service: string, limit?: number): Promise<TradeList> {
  return withService(service, () => getJson<TradeList>(bookPath("/api/book/trades", { service, limit })));
}

/** OHLCV buckets, `from`/`to` in unix seconds; `to` absent means now. */
export function fetchCandles(service: string, resolution: CandleResolution, from?: number, to?: number): Promise<CandleList> {
  return withService(service, () => getJson<CandleList>(bookPath("/api/book/candles", { service, resolution, from, to })));
}

/** The product's trading terms — readable by anyone who can see the product. */
export function fetchBookPolicy(service: string): Promise<BookPolicy> {
  return withService(service, () => getJson<BookPolicy>(bookPath("/api/book/policy", { service })));
}

// The three own-order reads take an OPTIONAL service: absent means every allocation. The
// terminal always names one, but a portfolio-wide list is the same route.

/** The caller's resting orders, oldest first. */
export function fetchOpenOrders(service?: string): Promise<OrderList> {
  return getJson<OrderList>(bookPath("/api/book/orders", { service }));
}

/** The caller's orders in every state, newest first. */
export function fetchOrderHistory(service?: string, limit?: number): Promise<OrderList> {
  return getJson<OrderList>(bookPath("/api/book/orders/history", { service, limit }));
}

/** The caller's own fills — with their side, their order and the fee they paid. */
export function fetchFills(service?: string, limit?: number): Promise<TradeList> {
  return getJson<TradeList>(bookPath("/api/book/fills", { service, limit }));
}

/** Place an order. The answer is the order as recorded — an IOC or market order that
 *  filled in part comes back already `cancelled` with `filled > 0`. */
export function placeOrder(body: PlaceOrderBody): Promise<Order> {
  return postJson<Order>("/api/book/orders", body);
}

export function cancelOrder(orderId: string): Promise<Order> {
  return postJson<Order>("/api/book/orders/cancel", { order_id: orderId });
}
