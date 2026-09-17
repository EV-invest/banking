"use client";

// One allocation's valuation log over a window, cached per (allocation, window start).
//
// Same window as `fundNavResource`: marks land only when an operator posts one, so five
// minutes of staleness costs nothing and a return to Home paints the curve it last drew.
// NOT persisted, unlike the NAV: the response carries the caller's own participation, and
// a holding is personal (see `ResourceConfig.persist`). Invalidated with the NAV — a new
// mark extends the fund line — and with the positions, since a subscription or a
// redemption is exactly what moves the participation line.

import { fetchFundNavHistory } from "@/entities/fund/api/fund-history-client";
import { TAG } from "@/shared/lib/cache-tags";
import { defineResource } from "@/shared/lib/resource";

export const fundNavHistoryResource = defineResource({
  name: "fund.navHistory",
  fetch: fetchFundNavHistory,
  key: (allocation, from) => `${allocation}@${from ?? "all"}`,
  revalidate: 300,
  tags: [TAG.nav, TAG.positions],
  enabled: (allocation) => allocation.trim().length > 0,
});
