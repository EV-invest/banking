// Browser → BFF operations client. One read: the caller's activity timeline, already
// merged and time-ordered by the hub. The shapes are the proto-derived types from
// `@/shared/contracts`. No tokens are ever seen here — the BFF holds them.

import { apiPath } from "@/shared/config/base-path";
import type { OperationList } from "@/shared/contracts";

export function fetchOperations(limit?: number): Promise<OperationList> {
  const query = limit ? `?limit=${limit}` : "";
  return getJson<OperationList>(`/api/operations${query}`);
}

async function getJson<T>(url: `/${string}`): Promise<T> {
  const res = await fetch(apiPath(url), { headers: { accept: "application/json" } });
  const data = (await res.json().catch(() => ({}))) as T & { error?: string };
  if (!res.ok) throw new Error(data.error ?? `request failed (${res.status})`);
  return data;
}
