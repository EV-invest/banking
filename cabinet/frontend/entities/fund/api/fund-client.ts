// Browser → BFF fund-shares client. Thin typed fetchers over the BFF route handlers;
// the shapes are the proto-derived types from `@/shared/contracts`. Mutations carry the
// CSRF double-submit header. No tokens are ever seen here — the BFF holds them.

import { apiPath } from "@/shared/config/base-path";
import { csrfHeader } from "@/shared/lib/csrf-client";
import type { FundNav, PositionList, Redemption, RedemptionList, Subscription } from "@/shared/contracts";

async function getJson<T>(url: `/${string}`): Promise<T> {
  const res = await fetch(apiPath(url), { headers: { accept: "application/json" } });
  const data = (await res.json().catch(() => ({}))) as T & { error?: string };
  if (!res.ok) throw new Error(data.error ?? `request failed (${res.status})`);
  return data;
}

export function fetchPositions(): Promise<PositionList> {
  return getJson<PositionList>("/api/funds/positions");
}

export function fetchFundNav(service: string): Promise<FundNav> {
  return getJson<FundNav>(`/api/funds/nav?service=${encodeURIComponent(service)}`);
}

export function fetchRedemptions(): Promise<RedemptionList> {
  return getJson<RedemptionList>("/api/funds/redemptions");
}

export async function submitSubscribe(body: { service: string; amount: string }): Promise<Subscription> {
  const res = await fetch(apiPath("/api/funds/subscribe"), {
    method: "POST",
    headers: { "content-type": "application/json", ...csrfHeader() },
    body: JSON.stringify(body),
  });
  const data = (await res.json().catch(() => ({}))) as Subscription & { error?: string };
  if (!res.ok) throw new Error(data.error ?? `subscribe failed (${res.status})`);
  return data;
}

export async function submitRedeem(body: { service: string; units: string }): Promise<Redemption> {
  const res = await fetch(apiPath("/api/funds/redeem"), {
    method: "POST",
    headers: { "content-type": "application/json", ...csrfHeader() },
    body: JSON.stringify(body),
  });
  const data = (await res.json().catch(() => ({}))) as Redemption & { error?: string };
  if (!res.ok) throw new Error(data.error ?? `redeem failed (${res.status})`);
  return data;
}

export async function cancelRedemption(redemptionId: string): Promise<Redemption> {
  const res = await fetch(apiPath("/api/funds/redemptions/cancel"), {
    method: "POST",
    headers: { "content-type": "application/json", ...csrfHeader() },
    body: JSON.stringify({ redemption_id: redemptionId }),
  });
  const data = (await res.json().catch(() => ({}))) as Redemption & { error?: string };
  if (!res.ok) throw new Error(data.error ?? `cancel failed (${res.status})`);
  return data;
}
