"use client";

// The book reads, cached the way every other screen's are, plus the two mutations that
// move them. Views import from here, not from `../api/book-client`.
//
// Two of these — the snapshot and the tape — are unusual: their normal delivery is the
// socket (`./book-socket`), which writes each frame straight into the cache with
// `publish`. The two-second window is the FALLBACK cadence, what the socket store polls at
// when the feed is down; while the socket is live nothing here ever asks the network. The
// terminal reads them through `useResource` either way, so it cannot tell — and does not
// need to — which path a figure arrived by.

import { cancelOrder as cancelOrderRequest, fetchBook, fetchBookPolicy, fetchFills, fetchOpenOrders, fetchOrderHistory, fetchTrades, placeOrder as placeOrderRequest } from "@/entities/book/api/book-client";
import type { Order, PlaceOrderBody } from "@/shared/contracts/book";
import { TAG } from "@/shared/lib/cache-tags";
import { defineResource, revalidateTag } from "@/shared/lib/resource";

/** Levels per side the terminal shows; the socket subscribes at the same depth so a frame
 *  and a poll paint the same book. */
export const BOOK_DEPTH = 12;
/** Prints kept on the tape. */
export const TAPE_LENGTH = 40;
/** Rows of own history and fills — the tabs are a glance, not an audit. */
export const HISTORY_LENGTH = 50;

const named = (service: string) => service.trim().length > 0;

// Operator-set terms; product-level, not personal — safe to mirror into sessionStorage so
// the product page decides whether to offer "Trade" on its first frame.
export const bookPolicyResource = defineResource({
  name: "book.policy",
  fetch: fetchBookPolicy,
  key: (service) => service,
  revalidate: 60,
  tags: [TAG.bookPolicy],
  persist: true,
  enabled: named,
});

export const bookSnapshotResource = defineResource({
  name: "book.snapshot",
  fetch: (service: string) => fetchBook(service, BOOK_DEPTH),
  key: (service) => service,
  revalidate: 2,
  tags: [TAG.book],
  enabled: named,
});

export const bookTradesResource = defineResource({
  name: "book.trades",
  fetch: (service: string) => fetchTrades(service, TAPE_LENGTH),
  key: (service) => service,
  revalidate: 2,
  tags: [TAG.book],
  enabled: named,
});

// The caller's own orders. Refreshed when the socket says `orders_revision` moved, and
// after every mutation below — the window is only the floor under both.
export const openOrdersResource = defineResource({
  name: "book.openOrders",
  fetch: (service: string) => fetchOpenOrders(service),
  key: (service) => service,
  revalidate: 15,
  tags: [TAG.orders],
  enabled: named,
});

export const orderHistoryResource = defineResource({
  name: "book.orderHistory",
  fetch: (service: string) => fetchOrderHistory(service, HISTORY_LENGTH),
  key: (service) => service,
  revalidate: 15,
  tags: [TAG.orders],
  enabled: named,
});

export const fillsResource = defineResource({
  name: "book.fills",
  fetch: (service: string) => fetchFills(service, HISTORY_LENGTH),
  key: (service) => service,
  revalidate: 15,
  tags: [TAG.orders],
  enabled: named,
});

/**
 * What a change to the caller's orders moves: the order lists themselves, and — because
 * an order is an escrow inside the ledger — the wallet's available balance and the
 * position's free units. Named once here so the socket and the mutations agree.
 */
export function refreshOwnOrders(): void {
  revalidateTag(TAG.orders, TAG.wallet, TAG.positions);
}

/** Place an order. Escrows cash (buy) or units (sell) the moment it is accepted. */
export async function placeOrder(body: PlaceOrderBody): Promise<Order> {
  const order = await placeOrderRequest(body);
  refreshOwnOrders();
  return order;
}

/** Cancel a resting order. Releases what it escrowed. */
export async function cancelOrder(orderId: string): Promise<Order> {
  const order = await cancelOrderRequest(orderId);
  refreshOwnOrders();
  return order;
}
