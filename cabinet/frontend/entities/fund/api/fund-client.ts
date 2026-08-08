// Browser → BFF fund-shares client. Thin typed fetchers over the BFF route handlers;
// the shapes are the proto-derived types from `@/shared/contracts`. Transport, CSRF and
// session handling belong to `@/shared/lib/api-client`. No tokens are ever seen here —
// the BFF holds them.

import { getJson, postJson } from "@/shared/lib/api-client";
import type { FundNav, PositionList, Redemption, RedemptionList, Subscription } from "@/shared/contracts";

export function fetchPositions(): Promise<PositionList> {
  return getJson<PositionList>("/api/funds/positions");
}

export function fetchFundNav(service: string): Promise<FundNav> {
  // Never issue a bare `/api/funds/nav` — the BFF 400s without ?service, so fail fast here.
  if (!service.trim()) return Promise.reject(new Error("fund service required"));
  return getJson<FundNav>(`/api/funds/nav?service=${encodeURIComponent(service)}`);
}

export function fetchRedemptions(): Promise<RedemptionList> {
  return getJson<RedemptionList>("/api/funds/redemptions");
}

export function submitSubscribe(body: { service: string; amount: string }): Promise<Subscription> {
  return postJson<Subscription>("/api/funds/subscribe", body);
}

export function submitRedeem(body: { service: string; units: string }): Promise<Redemption> {
  return postJson<Redemption>("/api/funds/redeem", body);
}

export function cancelRedemption(redemptionId: string): Promise<Redemption> {
  return postJson<Redemption>("/api/funds/redemptions/cancel", { redemption_id: redemptionId });
}
