// Browser → BFF admin-console client. Thin typed fetchers over the `/api/admin/*`
// routes; the shapes match the BFF DTOs (`@/shared/contracts/admin`). Mutations carry
// the CSRF double-submit header. No tokens are seen here — the BFF holds them and the
// owning plane re-checks the operator's role.

import { apiPath } from "@/shared/config/base-path";
import { csrfHeader } from "@/shared/lib/csrf-client";
import type { AdminOverview, AdminUserList, AdminUserProfile, CabinetConfig, FundNav, OperationsMode, ParkedEventList, PlatformConfig, Redemption, RedemptionQueue, Treasury, UserBalance } from "@/shared/contracts/admin";

async function getJson<T>(url: `/${string}`): Promise<T> {
  const res = await fetch(apiPath(url), { headers: { accept: "application/json" } });
  const data = (await res.json().catch(() => ({}))) as T & { error?: string };
  if (!res.ok) throw new Error(data.error ?? `request failed (${res.status})`);
  return data;
}

async function postJson<T>(url: `/${string}`, body: unknown): Promise<T> {
  const res = await fetch(apiPath(url), {
    method: "POST",
    headers: { "content-type": "application/json", ...csrfHeader() },
    body: JSON.stringify(body),
  });
  const data = (await res.json().catch(() => ({}))) as T & { error?: string };
  if (!res.ok) throw new Error(data.error ?? `request failed (${res.status})`);
  return data;
}

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

// ── valuation + redemptions ─────────────────────────────────────────────────────
export const fetchRedemptionQueue = (): Promise<RedemptionQueue> => getJson("/api/admin/valuation/queue");

export const postValuation = (body: { service: string; aum: string; override: boolean }): Promise<FundNav> => postJson("/api/admin/valuation/post", body);

export const settleRedemption = (redemptionId: string): Promise<Redemption> => postJson("/api/admin/valuation/settle", { redemption_id: redemptionId });

export const failRedemption = (redemptionId: string): Promise<Redemption> => postJson("/api/admin/valuation/fail", { redemption_id: redemptionId });

// ── cabinet (platform config + read-only kill-switch) ───────────────────────────
export const fetchCabinet = (): Promise<CabinetConfig> => getJson("/api/admin/cabinet");

export const setMaintenance = (enabled: boolean): Promise<PlatformConfig> => postJson("/api/admin/cabinet/maintenance", { enabled });

export const setReadOnly = (readOnly: boolean): Promise<OperationsMode> => postJson("/api/admin/cabinet/read-only", { read_only: readOnly });

export const setAnnouncement = (body: { title: string; body: string; active: boolean }): Promise<PlatformConfig> => postJson("/api/admin/cabinet/announcement", body);

export const setFeatureFlag = (body: { key: string; description: string; enabled: boolean; rollout: number }): Promise<PlatformConfig> => postJson("/api/admin/cabinet/flag", body);
