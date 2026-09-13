"use client";

// The payments read, and the two mutations that move it.
//
// Mutations invalidate rather than publish, for the reason `entities/governance` gives: an
// order's state is settled by the plane under a row lock, and what the list shows should be
// the plane's answer, not the response one click happened to receive. Every write names
// three tags — an order that opens a consilium changes the owners' room, and one that
// executes debits a claim the treasury screen shows.

import {
  cancelPayment as cancelPaymentRequest,
  fetchPayments,
  openPayment as openPaymentRequest,
} from "@/entities/payment/api/payment-client";
import type { OpenPaymentRequest, Payment } from "@/shared/contracts/payments";
import { TAG } from "@/shared/lib/cache-tags";
import { defineResource, revalidateTag } from "@/shared/lib/resource";

/** Every tag a payment write can move. */
export const PAYMENT_TAGS = [TAG.payments, TAG.consilium, TAG.adminTreasury] as const;

/** Keyed on the state filter: a filtered list is a different question, answered server-side. */
export const paymentsResource = defineResource({
  name: "payments.list",
  fetch: (state?: string) => fetchPayments(state),
  key: (state?: string) => state ?? "",
  revalidate: 15,
  tags: [TAG.payments],
});

export async function openPayment(body: OpenPaymentRequest): Promise<Payment> {
  const payment = await openPaymentRequest(body);
  revalidateTag(...PAYMENT_TAGS);
  return payment;
}

export async function cancelPayment(paymentId: string): Promise<Payment> {
  const payment = await cancelPaymentRequest(paymentId);
  revalidateTag(...PAYMENT_TAGS);
  return payment;
}
