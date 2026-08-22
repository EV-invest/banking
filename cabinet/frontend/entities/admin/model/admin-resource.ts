"use client";

// The operator console's reads, cached the same way the investor screens' are, so moving
// between Overview, Users, Treasury and the queues doesn't re-fetch each one from scratch.
//
// Shorter windows than the investor side: these are operational screens, and an operator
// looking at a queue is looking at it *now*. Cached data still paints first — the refresh
// happens behind it — so the effect is a screen that opens on last-known state and settles,
// rather than one that opens empty.
//
// Mutations stay where they are, in the views: each admin action already owns a deliberate
// refetch-and-reconcile (unpark marks a row before re-reading, a failed dispatch must not
// clear the queue). Those flows call `refresh()` or `invalidate()` on the resource instead
// of re-fetching by hand, which keeps every other open surface in step for free.

import {
  fetchAllocations as fetchAdminAllocations,
  fetchCabinet,
  fetchFundRevenue,
  fetchMfeRegistry,
  fetchOverview,
  fetchParkedEvents,
  fetchFeeAssessments,
  fetchFeePolicies,
  fetchFeeShares,
  fetchRedemptionQueue,
  fetchRevenuePayouts,
  fetchTreasury,
  fetchUser,
  fetchUserBalance,
  fetchUsers,
  fetchWithdrawalQueue,
  type UserFilters,
} from "@/entities/admin/api/admin-client";
import { TAG } from "@/shared/lib/cache-tags";
import { defineResource } from "@/shared/lib/resource";

const OPERATIONAL = 10;

export const overviewResource = defineResource({
  name: "admin.overview",
  fetch: fetchOverview,
  revalidate: OPERATIONAL,
  tags: [TAG.adminFleet],
});

export const parkedEventsResource = defineResource({
  name: "admin.parked",
  fetch: fetchParkedEvents,
  revalidate: OPERATIONAL,
  tags: [TAG.adminFleet],
});

export const treasuryResource = defineResource({
  name: "admin.treasury",
  fetch: fetchTreasury,
  revalidate: OPERATIONAL,
  tags: [TAG.adminTreasury],
});

export const cabinetConfigResource = defineResource({
  name: "admin.cabinet",
  fetch: fetchCabinet,
  revalidate: 60,
  tags: [TAG.adminCabinet],
});

// Deployment config, not account data — safe to mirror into sessionStorage, and it changes
// only when a remote is redeployed.
export const mfeRegistryResource = defineResource({
  name: "admin.mfeRegistry",
  fetch: fetchMfeRegistry,
  revalidate: 300,
  tags: [TAG.adminCabinet],
  persist: true,
});

export const withdrawalQueueResource = defineResource({
  name: "admin.withdrawalQueue",
  fetch: fetchWithdrawalQueue,
  revalidate: OPERATIONAL,
  tags: [TAG.adminQueue],
});

// The fund's own money. Operational cadence like the queues — an owner looking at what
// is payable is looking at it now, and a settling payout moves the figure.
export const fundRevenueResource = defineResource({
  name: "admin.fundRevenue",
  fetch: fetchFundRevenue,
  revalidate: OPERATIONAL,
  tags: [TAG.adminRevenue],
});

export const revenuePayoutsResource = defineResource({
  name: "admin.revenuePayouts",
  fetch: fetchRevenuePayouts,
  revalidate: OPERATIONAL,
  tags: [TAG.adminRevenue],
});

// Fee terms change only when an operator rewrites them — registry cadence, not
// operational — so the table opens on what it last showed.
export const feePoliciesResource = defineResource({
  name: "admin.feePolicies",
  fetch: fetchFeePolicies,
  revalidate: 300,
  tags: [TAG.adminFees],
});

// The accumulated units move with every sweep, and their value moves with every mark, so
// this is operational: an operator deciding whether to settle is looking at it now.
export const feeSharesResource = defineResource({
  name: "admin.feeShares",
  fetch: fetchFeeShares,
  key: (service) => service,
  revalidate: OPERATIONAL,
  tags: [TAG.adminFees],
  enabled: (service) => service.trim().length > 0,
});

export const feeAssessmentsResource = defineResource({
  name: "admin.feeAssessments",
  fetch: fetchFeeAssessments,
  key: (service) => service,
  revalidate: OPERATIONAL,
  tags: [TAG.adminFees],
  enabled: (service) => service.trim().length > 0,
});

export const redemptionQueueResource = defineResource({
  name: "admin.redemptionQueue",
  fetch: fetchRedemptionQueue,
  revalidate: OPERATIONAL,
  tags: [TAG.adminQueue],
});

// The operator listing — drafts and closed products included, unlike the investor-facing
// `/api/allocations` the rail reads. Two different questions, two different cache entries.
export const adminAllocationsResource = defineResource({
  name: "admin.allocations",
  fetch: fetchAdminAllocations,
  revalidate: 30,
  tags: [TAG.adminAllocations],
});

// Keyed on the whole filter set: a search is a different question, not a refinement of the
// last one, and the hub applies `query`/`role`/`status` server-side.
export const usersResource = defineResource({
  name: "admin.users",
  fetch: fetchUsers,
  key: (filters: UserFilters = {}) => JSON.stringify([filters.query ?? "", filters.role ?? "", filters.status ?? "", filters.limit ?? 0, filters.offset ?? 0]),
  revalidate: OPERATIONAL,
  tags: [TAG.adminUsers],
});

export const adminUserResource = defineResource({
  name: "admin.user",
  fetch: fetchUser,
  key: (userId) => userId,
  revalidate: OPERATIONAL,
  tags: [TAG.adminUsers],
  enabled: (userId) => userId.length > 0,
});

export const adminUserBalanceResource = defineResource({
  name: "admin.userBalance",
  fetch: fetchUserBalance,
  key: (userId) => userId,
  revalidate: OPERATIONAL,
  tags: [TAG.adminUsers],
  enabled: (userId) => userId.length > 0,
});
