"use client";

// Payment orders, over the ordinary signed-in transport. The token-and-code consent an
// investor answers from their mailbox is the separate, session-free `entities/approval`
// client — same split as the owners' room and its emailed approvals.

import type { OpenPaymentRequest, Payment, PaymentList } from "@/shared/contracts/payments";
import { getJson, postJson } from "@/shared/lib/api-client";

/** Every order the caller may see; `state` narrows to one lifecycle state. */
export function fetchPayments(state?: string, limit?: number): Promise<PaymentList> {
  const params = new URLSearchParams();
  if (state) params.set("state", state);
  if (limit) params.set("limit", String(limit));
  const qs = params.toString();
  return getJson<PaymentList>(`/api/admin/payments${qs ? `?${qs}` : ""}`);
}

export function fetchPayment(paymentId: string): Promise<Payment> {
  return getJson<Payment>(`/api/admin/payments/${encodeURIComponent(paymentId)}`);
}

/**
 * Open an order. What comes back already says who has been asked: `consilium_id` for
 * fund-owned money, `consent` for an investor's own claim. Neither is chosen here — the
 * plane derives both, and the form's preview of them is a courtesy, not the control.
 */
export function openPayment(body: OpenPaymentRequest): Promise<Payment> {
  return postJson<Payment>("/api/admin/payments", body);
}

/** Initiator only, and only while pending. Voids whatever approvals were collected. */
export function cancelPayment(paymentId: string): Promise<Payment> {
  return postJson<Payment>(`/api/admin/payments/${encodeURIComponent(paymentId)}/cancel`, {});
}
