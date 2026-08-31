// Browser → BFF fund-shares client. Thin typed fetchers over the BFF route handlers;
// the shapes are the proto-derived types from `@/shared/contracts`. Transport, CSRF and
// session handling belong to `@/shared/lib/api-client`. No tokens are ever seen here —
// the BFF holds them.
//
// The fail-fast guards below reject with a `RequestError` rather than a bare `Error`, for
// the same reason the transport does: the rejection ends up on screen through
// `ResourceError`, and only a keyed error can be read there in the reader's language. The
// status is 400 because that is what the BFF would have answered had the call gone out.

import { getJson, postJson, RequestError } from "@/shared/lib/api-client";
import type { AccruedFees, AllocationList, FeePolicy, FundNav, PositionList, Redemption, RedemptionList, Subscription } from "@/shared/contracts";

/// The investor-facing catalog: `open` allocations only. Subscribing to anything else is
/// refused by the hub, so this is the only honest source for the fund picker.
export function fetchAllocations(): Promise<AllocationList> {
  return getJson<AllocationList>("/api/allocations");
}

export function fetchPositions(): Promise<PositionList> {
  return getJson<PositionList>("/api/funds/positions");
}

export function fetchFundNav(service: string): Promise<FundNav> {
  // Never issue a bare `/api/funds/nav` — the BFF 400s without ?service, so fail fast here.
  if (!service.trim()) return Promise.reject(new RequestError("fund service required", 400, "err.fundServiceRequired"));
  return getJson<FundNav>(`/api/funds/nav?service=${encodeURIComponent(service)}`);
}

/// What this fund charges. Readable whether or not the caller holds it — the terms are
/// part of deciding to buy in, not a detail revealed afterwards.
export function fetchFeePolicy(service: string): Promise<FeePolicy> {
  if (!service.trim()) return Promise.reject(new RequestError("fund service required", 400, "err.fundServiceRequired"));
  return getJson<FeePolicy>(`/api/funds/fee-policy?service=${encodeURIComponent(service)}`);
}

/// What the caller's holding owes right now, uncharged — the figure `value` has to be
/// read net of.
export function fetchAccruedFees(service: string): Promise<AccruedFees> {
  if (!service.trim()) return Promise.reject(new RequestError("fund service required", 400, "err.fundServiceRequired"));
  return getJson<AccruedFees>(`/api/funds/accrued-fees?service=${encodeURIComponent(service)}`);
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
