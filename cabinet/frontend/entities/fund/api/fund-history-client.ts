// Browser → BFF client for one allocation's valuation log. Same rules as `./fund-client`:
// transport, CSRF and session live in `@/shared/lib/api-client`, the shape is the
// proto-derived type, and a request the BFF would 400 is refused here with a keyed
// `RequestError` so the screen can say why in the reader's language.

import type { FundNavHistory } from "@/shared/contracts";
import { getJson, RequestError } from "@/shared/lib/api-client";

/// The posted marks of one allocation over `[from, to]` (unix seconds; either bound
/// absent = all-time / now) plus the caller's own participation through them. Keyed by
/// the allocation rather than the service (#245) — today the two are the same string.
export function fetchFundNavHistory(allocation: string, from?: number, to?: number): Promise<FundNavHistory> {
  if (!allocation.trim()) return Promise.reject(new RequestError("fund service required", 400, "err.fundServiceRequired"));
  const query = new URLSearchParams({ allocation });
  if (from !== undefined) query.set("from", String(from));
  if (to !== undefined) query.set("to", String(to));
  return getJson<FundNavHistory>(`/api/funds/nav/history?${query}`);
}
