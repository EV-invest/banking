// Browser → BFF operations client. One read: the caller's activity timeline, already
// merged and time-ordered by the hub. The shapes are the proto-derived types from
// `@/shared/contracts`. Transport, CSRF and session handling belong to
// `@/shared/lib/api-client`. No tokens are ever seen here — the BFF holds them.

import type { OperationList } from "@/shared/contracts";
import { getJson } from "@/shared/lib/api-client";

export function fetchOperations(limit?: number): Promise<OperationList> {
  const query = limit ? `?limit=${limit}` : "";
  return getJson<OperationList>(`/api/operations${query}`);
}
