"use client";

// The fund reads, cached once for every screen that shows them, plus the three mutations
// that move them. Views import from here, not from `../api/fund-client`.
//
// The catalog is the one read that is neither personal nor volatile: it is the operator's
// registry of open funds, it is asked for by the rail on EVERY signed-in screen, and it is
// what names a fund in Home's activity rows, in Operations, and on the product page. It gets
// a five-minute window and a sessionStorage mirror, so a reload paints the rail's product
// list before the network answers.

import { cancelRedemption as cancelRedemptionRequest, fetchAllocations, fetchFundNav, fetchPositions, fetchRedemptions, submitRedeem as submitRedeemRequest, submitSubscribe as submitSubscribeRequest } from "@/entities/fund/api/fund-client";
import type { Redemption, Subscription } from "@/shared/contracts";
import { TAG } from "@/shared/lib/cache-tags";
import { defineResource, revalidateTag } from "@/shared/lib/resource";

export const allocationsResource = defineResource({
  name: "fund.allocations",
  fetch: fetchAllocations,
  revalidate: 300,
  tags: [TAG.catalog],
  persist: true,
});

export const positionsResource = defineResource({
  name: "fund.positions",
  fetch: fetchPositions,
  tags: [TAG.positions],
});

export const redemptionsResource = defineResource({
  name: "fund.redemptions",
  fetch: fetchRedemptions,
  tags: [TAG.redemptions],
});

// NAV moves only when an operator posts a valuation, so a five-minute window costs nothing
// and a fund's page opens on the price it last showed. Fund-level market data, not the
// caller's — safe to mirror into sessionStorage. `enabled` keeps a screen that hasn't picked
// a fund yet from issuing the bare request the BFF answers with a 400.
export const fundNavResource = defineResource({
  name: "fund.nav",
  fetch: fetchFundNav,
  key: (service) => service,
  revalidate: 300,
  tags: [TAG.nav],
  persist: true,
  enabled: (service) => service.trim().length > 0,
});

/** Buy into a fund: money leaves the wallet and becomes units. */
export async function submitSubscribe(body: { service: string; amount: string }): Promise<Subscription> {
  const subscription = await submitSubscribeRequest(body);
  revalidateTag(TAG.wallet, TAG.positions, TAG.operations);
  return subscription;
}

/** Sell units back. Nothing settles until an operator prices it, so the wallet is unmoved. */
export async function submitRedeem(body: { service: string; units: string }): Promise<Redemption> {
  const redemption = await submitRedeemRequest(body);
  revalidateTag(TAG.positions, TAG.redemptions, TAG.operations);
  return redemption;
}

/** Withdraw a redemption request that hasn't been priced yet. */
export async function cancelRedemption(redemptionId: string): Promise<Redemption> {
  const cancelled = await cancelRedemptionRequest(redemptionId);
  revalidateTag(TAG.positions, TAG.redemptions, TAG.operations);
  return cancelled;
}
