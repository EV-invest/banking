// Browser → BFF admin-console client. Thin typed fetchers over the `/api/admin/*`
// routes; the shapes match the BFF DTOs (`@/shared/contracts/admin`). Mutations carry
// the CSRF double-submit header. No tokens are seen here — the BFF holds them and the
// owning plane re-checks the operator's role.

import { getJson, postJson } from "@/shared/lib/api-client";
import type {
  AdminOverview,
  Allocation,
  AllocationList,
  AdminUserList,
  AdminUserProfile,
  CabinetConfig,
  FundNav,
  OperationsMode,
  ParkedEventList,
  PlatformConfig,
  Redemption,
  RedemptionQueue,
  Treasury,
  UserBalance,
  WithdrawalQueue,
} from "@/shared/contracts/admin";

// ── overview ──────────────────────────────────────────────────────────────────
export const fetchOverview = (): Promise<AdminOverview> => getJson("/api/admin/overview");

// ── outbox ────────────────────────────────────────────────────────────────────
export const fetchParkedEvents = (): Promise<ParkedEventList> => getJson("/api/admin/outbox/parked");

export const unparkEvent = (seq: string): Promise<{ ok: boolean }> => postJson("/api/admin/outbox/unpark", { seq });

// ── users ─────────────────────────────────────────────────────────────────────
export interface UserFilters {
  query?: string;
  role?: string;
  status?: string;
  limit?: number;
  offset?: number;
}

export function fetchUsers(filters: UserFilters = {}): Promise<AdminUserList> {
  const params = new URLSearchParams();
  if (filters.query) params.set("query", filters.query);
  if (filters.role) params.set("role", filters.role);
  if (filters.status) params.set("status", filters.status);
  if (filters.limit) params.set("limit", String(filters.limit));
  if (filters.offset) params.set("offset", String(filters.offset));
  const qs = params.toString();
  return getJson(`/api/admin/users${qs ? `?${qs}` : ""}`);
}

export const fetchUser = (userId: string): Promise<AdminUserProfile> => getJson(`/api/admin/users/detail?user_id=${encodeURIComponent(userId)}`);

export const fetchUserBalance = (userId: string): Promise<UserBalance> => getJson(`/api/admin/users/balance?user_id=${encodeURIComponent(userId)}`);

export const setUserRole = (userId: string, role: string): Promise<{ role: string }> => postJson("/api/admin/users/role", { user_id: userId, role });

export const suspendUser = (userId: string): Promise<{ ok: boolean }> => postJson("/api/admin/users/suspend", { user_id: userId });

export const reinstateUser = (userId: string): Promise<{ ok: boolean }> => postJson("/api/admin/users/reinstate", { user_id: userId });

export const revokeSessions = (userId: string): Promise<{ token_version: string }> => postJson("/api/admin/users/revoke", { user_id: userId });

export const setKycLevel = (userId: string, kycLevel: number): Promise<{ kyc_level: number }> => postJson("/api/admin/users/kyc", { user_id: userId, kyc_level: kycLevel });

// ── treasury ──────────────────────────────────────────────────────────────────
export const fetchTreasury = (): Promise<Treasury> => getJson("/api/admin/treasury");

/** Record USDT that reached a rail out of band (an operator top-up of the treasury hot
 *  wallet), which otherwise moves real money without ever touching the ledger.
 *
 *  The amount and the credited party are read off the CHAIN, not sent — this cannot mint a
 *  balance. `expected_amount` is an optional assertion the chain must match. Idempotent by
 *  `tx_ref`: `recorded: false` means that reference was already credited. */
export const recordTreasuryDeposit = (body: { tx_ref: string; network: string; expected_amount?: string }): Promise<RecordedArrival> =>
  postJson("/api/admin/treasury/record-deposit", body);

export type RecordedArrival = { recorded: boolean; amount: string; party_kind: string; party_id: string };

// ── allocations (the registry — the only way a fund comes into existence) ────────
// The admin listing includes drafts and closed products; the investor-facing
// `/api/allocations` returns the open set only.
export const fetchAllocations = (): Promise<AllocationList> => getJson("/api/admin/allocations");

export const registerAllocation = (body: { service: string; title: string; summary: string }): Promise<Allocation> =>
  postJson("/api/admin/allocations/register", body);

export const updateAllocation = (body: { service: string; title: string; summary: string }): Promise<Allocation> => postJson("/api/admin/allocations/update", body);

export const setAllocationState = (service: string, state: "open" | "closed"): Promise<Allocation> => postJson("/api/admin/allocations/state", { service, state });

// Its own route rather than a field on `/update`: the cap gates money, so the hub raises
// its own audit event for it and refuses a subscription that would mint past it.
export const setAllocationUnitCap = (service: string, unitCap: string): Promise<Allocation> => postJson("/api/admin/allocations/cap", { service, unit_cap: unitCap });

// ── valuation + redemptions ─────────────────────────────────────────────────────
export const fetchRedemptionQueue = (): Promise<RedemptionQueue> => getJson("/api/admin/valuation/queue");

export const postValuation = (body: { service: string; aum: string; override: boolean }): Promise<FundNav> => postJson("/api/admin/valuation/post", body);

export const settleRedemption = (redemptionId: string): Promise<Redemption> => postJson("/api/admin/valuation/settle", { redemption_id: redemptionId });

export const failRedemption = (redemptionId: string): Promise<Redemption> => postJson("/api/admin/valuation/fail", { redemption_id: redemptionId });

// ── withdrawals (operator queue + actions) ───────────────────────────────────────
export const fetchWithdrawalQueue = (): Promise<WithdrawalQueue> => getJson("/api/admin/withdrawals/queue");

export const dispatchWithdrawal = (withdrawalId: string): Promise<{ ok: boolean }> => postJson("/api/admin/withdrawals/dispatch", { withdrawal_id: withdrawalId });

export const settleWithdrawal = (withdrawalId: string, txRef: string): Promise<{ ok: boolean }> =>
  postJson("/api/admin/withdrawals/settle", { withdrawal_id: withdrawalId, tx_ref: txRef });

export const failWithdrawal = (withdrawalId: string, reason: string): Promise<{ ok: boolean }> =>
  postJson("/api/admin/withdrawals/fail", { withdrawal_id: withdrawalId, reason });

// ── cabinet (platform config + read-only kill-switch) ───────────────────────────
export const fetchCabinet = (): Promise<CabinetConfig> => getJson("/api/admin/cabinet");

export const setMaintenance = (enabled: boolean): Promise<PlatformConfig> => postJson("/api/admin/cabinet/maintenance", { enabled });

export const setReadOnly = (readOnly: boolean): Promise<OperationsMode> => postJson("/api/admin/cabinet/read-only", { read_only: readOnly });

export const setAnnouncement = (body: { title: string; body: string; active: boolean }): Promise<PlatformConfig> => postJson("/api/admin/cabinet/announcement", body);

export const setFeatureFlag = (body: { key: string; description: string; enabled: boolean; rollout: number }): Promise<PlatformConfig> => postJson("/api/admin/cabinet/flag", body);
