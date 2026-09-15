// Browser → BFF admin-console client. Thin typed fetchers over the `/api/admin/*`
// routes; the shapes match the BFF DTOs (`@/shared/contracts/admin`). Mutations carry
// the CSRF double-submit header. No tokens are seen here — the BFF holds them and the
// owning plane re-checks the operator's role.

import { getJson, postJson } from "@/shared/lib/api-client";
import type {
  AdminOverview,
  Allocation,
  AllocationAccessGrant,
  AllocationAccessGrantList,
  AllocationAccessLevel,
  AllocationBacking,
  AllocationGrantLevel,
  AllocationIcon,
  AllocationList,
  AdminUserList,
  AdminUserProfile,
  CancelFeePolicyChangeRequest,
  FeeAssessmentList,
  FeePolicyChange,
  FeePolicyChangeList,
  FeePolicyList,
  FeeSettlement,
  FeeShares,
  ScheduleFeePolicyRequest,
  CabinetConfig,
  FundNav,
  FundRevenue,
  OperationsMode,
  ParkedEventList,
  PlatformConfig,
  Redemption,
  RedemptionQueue,
  RetireUnitsBody,
  RevenuePayout,
  RevenuePayoutList,
  TransferStakeBody,
  Treasury,
  UnitHolders,
  UnitIssuance,
  UserBalance,
  WithdrawalQueue,
} from "@/shared/contracts/admin";
import type { BookPolicy, SetBookPolicyBody } from "@/shared/contracts/book";
import type { Consilium } from "@/shared/contracts/governance";
import type { MfeEntry } from "@/shared/mfe/types";

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

/**
 * The emergency brake: freeze the account NOW. It lapses on its own after 24h unless the
 * owners ratify it, and the answer is WHEN — `hold_expires_at`, unix seconds as a string.
 *
 * This replaces `suspendUser`, which posted to `DisableUser`: the identity plane now always
 * refuses that verb, because it was both this instant freeze and a permanent verdict one
 * person made with no record of why. Making an account stay blocked is the separate
 * {@link openUserSuspension} in `entities/governance`.
 *
 * `reason` is required, and not by this console's choice — it is what the owners asked to
 * ratify the hold are reading, and the plane refuses a hold without one.
 */
export const holdUser = (userId: string, reason: string): Promise<{ hold_expires_at: string }> => postJson("/api/admin/users/hold", { user_id: userId, reason });

/**
 * Lift a HOLD, in one act.
 *
 * Deliberately takes no reason: this is the undo of a recorded action, and an undo that
 * demands a sentence first is friction on the path that makes someone's money work again.
 * The plane refuses this outright when the suspension was the owners' verdict — the console
 * reads `suspended_by` and offers {@link openUserReinstatement} instead of this.
 */
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

/** The register/update body. `icon` is required here even though it is optional on the
 *  read shape. The hub no longer needs it to be: `icon` carries presence on the wire, so
 *  an omitted field means "leave it alone" rather than "reset to `fund`". It stays
 *  required because this console always knows the icon it is editing, and a request that
 *  states every field it is writing is one you can read back later and know what it did.
 *  Send the current value even when only the title changed. */
export interface AllocationWrite {
  service: string;
  title: string;
  summary: string;
  icon: AllocationIcon;
}

export const registerAllocation = (body: AllocationWrite): Promise<Allocation> => postJson("/api/admin/allocations/register", body);

export const updateAllocation = (body: AllocationWrite): Promise<Allocation> => postJson("/api/admin/allocations/update", body);

export const setAllocationState = (service: string, state: "open" | "closed"): Promise<Allocation> => postJson("/api/admin/allocations/state", { service, state });

// Its own route rather than a field on `/update`: the cap gates money, so the hub raises
// its own audit event for it and refuses a subscription that would mint past it.
export const setAllocationUnitCap = (service: string, unitCap: string): Promise<Allocation> => postJson("/api/admin/allocations/cap", { service, unit_cap: unitCap });

// A product's default access — "closed by default": every registration lands on `view`
// (listed, locked) until an operator opens it up. Its own route for the same reason the
// unit cap has one: the hub raises its own audit event for an access change.
export const setAllocationAccess = (service: string, access: AllocationAccessLevel): Promise<Allocation> => postJson("/api/admin/allocations/access", { service, access });

// One investor raised above the product's default — `hidden` is refused here (grantable
// only, not the full vocabulary): a grant that lowered a holder below the default would
// make "revoke" ambiguous.
export const fetchAllocationAccessGrants = (service: string): Promise<AllocationAccessGrantList> =>
  getJson(`/api/admin/allocations/grants?service=${encodeURIComponent(service)}`);

export const grantAllocationAccess = (service: string, userId: string, level: AllocationGrantLevel): Promise<AllocationAccessGrant> =>
  postJson("/api/admin/allocations/grants/grant", { service, user_id: userId, level });

// Drops the investor back to the product's own default — not to `hidden`, which is why
// this is "revoke" and not "hide".
export const revokeAllocationAccess = (service: string, userId: string): Promise<Record<string, never>> =>
  postJson("/api/admin/allocations/grants/revoke", { service, user_id: userId });

/** The in-kind issue body, exactly as the BFF reads it: one of `user_id` or `company`
 *  (both is a 400), `cost_basis` present only when the operator typed one (absent means
 *  `units × NAV` hub-side — an empty string is NOT the same as absent). The two holder
 *  shapes are a union rather than two optional fields so a body naming both cannot be
 *  typed at all; `issueUnitsBody` in `views/admin/allocations/lib/issuance.ts` is the
 *  one place that builds it from a form. */
export type IssueUnitsBody = {
  service: string;
  /** Decimal units, > 0. */
  units: string;
  /** Decimal USDT the holder is deemed to have paid; omitted = `units × NAV`. */
  cost_basis?: string;
  /** 1..64 chars, unique per service. The retry contract: one key per submission, the
   *  same key on a retry of that submission, so a double click lands one mint. */
  idempotency_key: string;
} & ({ user_id: string; company?: never } | { company: true; user_id?: never });

export const issueUnits = (body: IssueUnitsBody): Promise<UnitIssuance> => postJson("/api/admin/allocations/issue", body);

// Hand part of the company's stake to an investor: the units leave the company's
// holding and land in theirs, and the supply does not move. Answers the same shape as a
// mint with `source: "company"`; the key shares the mint's per-product key space.
export const transferCompanyStake = (body: TransferStakeBody): Promise<UnitIssuance> => postJson("/api/admin/allocations/transfer-stake", body);

// The mirror of a mint: burn units out of one holder — an investor or the company — so
// the supply shrinks by them. Answers the same shape with `source: "retire"`; the key
// shares the mint's per-product key space. Allowed on a `closed` product, or on a live
// one only with the operator's explicit `force`.
export const retireUnits = (body: RetireUnitsBody): Promise<UnitIssuance> => postJson("/api/admin/allocations/retire", body);

// What stands behind the units. Its own route for the same reason the cap and the access
// level have one: it decides whether `Redeem` pays out or is refused, so the hub raises
// its own audit event for the flip.
export const setAllocationBacking = (service: string, backing: AllocationBacking): Promise<Allocation> => postJson("/api/admin/allocations/backing", { service, backing });

export const fetchUnitHolders = (service: string): Promise<UnitHolders> => getJson(`/api/admin/allocations/holders?service=${encodeURIComponent(service)}`);

// The product's secondary-market terms. Reading them is the investor route
// (`GET /api/book/policy`, `entities/book`); only the write is an operator's. The
// optional fields are sent only when set — absent means "keep the hub's value" — and
// `views/admin/allocations/lib/book-policy.ts` is the one place that builds the body.
export const setBookPolicy = (body: SetBookPolicyBody): Promise<BookPolicy> => postJson("/api/admin/allocations/book", body);

// ── valuation + redemptions ─────────────────────────────────────────────────────
export const fetchRedemptionQueue = (): Promise<RedemptionQueue> => getJson("/api/admin/valuation/queue");

export const postValuation = (body: { service: string; aum: string }): Promise<FundNav> => postJson("/api/admin/valuation/post", body);

// A mark the NAV-move guard refuses is not posted with a flag — it is put to the owners.
// The answer is the consilium the room and the emailed invitations then show.
export const proposeValuationOverride = (body: { service: string; aum: string }): Promise<Consilium> => postJson("/api/admin/valuation/override", body);

export const settleRedemption = (redemptionId: string): Promise<Redemption> => postJson("/api/admin/valuation/settle", { redemption_id: redemptionId });

export const failRedemption = (redemptionId: string): Promise<Redemption> => postJson("/api/admin/valuation/fail", { redemption_id: redemptionId });

// ── withdrawals (operator queue + actions) ───────────────────────────────────────
export const fetchWithdrawalQueue = (): Promise<WithdrawalQueue> => getJson("/api/admin/withdrawals/queue");

export const dispatchWithdrawal = (withdrawalId: string): Promise<{ ok: boolean }> => postJson("/api/admin/withdrawals/dispatch", { withdrawal_id: withdrawalId });

export const settleWithdrawal = (withdrawalId: string, txRef: string): Promise<{ ok: boolean }> =>
  postJson("/api/admin/withdrawals/settle", { withdrawal_id: withdrawalId, tx_ref: txRef });

export const failWithdrawal = (withdrawalId: string, reason: string): Promise<{ ok: boolean }> =>
  postJson("/api/admin/withdrawals/fail", { withdrawal_id: withdrawalId, reason });

// ── revenue (the fund's own earned money) ────────────────────────────────────────
// Reads, and cancelling a still-queued payout. Paying revenue OUT is a payment order
// (`entities/payment`) authorised by the owners' consilium; the one-click
// `RequestRevenuePayout` this client used to post to is closed at the plane.
export const fetchFundRevenue = (): Promise<FundRevenue> => getJson("/api/admin/revenue");

export const fetchRevenuePayouts = (): Promise<RevenuePayoutList> => getJson("/api/admin/revenue/payouts");

export const cancelRevenuePayout = (withdrawalId: string): Promise<RevenuePayout> => postJson("/api/admin/revenue/cancel", { withdrawal_id: withdrawalId });

// ── fees ────────────────────────────────────────────────────────────────────────
export const fetchFeePolicies = (): Promise<FeePolicyList> => getJson("/api/admin/fees/policies");

export const fetchFeeShares = (service: string): Promise<FeeShares> => getJson(`/api/admin/fees/shares?service=${encodeURIComponent(service)}`);

export const fetchFeeAssessments = (service: string): Promise<FeeAssessmentList> => getJson(`/api/admin/fees/assessments?service=${encodeURIComponent(service)}`);

/** Propose a change of a fund's terms. The hub decides who must agree and when the change
 *  binds; the answer says both (`state`, `requirement`, `effective_from`). A second request
 *  while one is pending is refused until the first is cancelled. */
export const scheduleFeePolicy = (body: ScheduleFeePolicyRequest): Promise<FeePolicyChange> => postJson("/api/admin/fees/policy", body);

export const cancelFeePolicyChange = (body: CancelFeePolicyChangeRequest): Promise<FeePolicyChange> => postJson("/api/admin/fees/policy/cancel", body);

/** A fund's whole history of terms, newest version first. */
export const fetchFeePolicyChanges = (service: string): Promise<FeePolicyChangeList> => getJson(`/api/admin/fees/changes?service=${encodeURIComponent(service)}`);

// Empty `units` settles the whole accumulated balance — the ordinary end-of-period call.
export const settleFeeShares = (body: { service: string; units: string }): Promise<FeeSettlement> => postJson("/api/admin/fees/settle", body);

// ── cabinet (platform config + read-only kill-switch) ───────────────────────────
export const fetchCabinet = (): Promise<CabinetConfig> => getJson("/api/admin/cabinet");

// Deployment config, not account data: the same list every client resolves its remotes
// from. A non-array answer is treated as an empty registry rather than a failure — the
// console's other panels must stay useful when the registry file is missing.
export const fetchMfeRegistry = async (): Promise<MfeEntry[]> => {
  const entries = await getJson<MfeEntry[]>("/api/mfe-registry");
  return Array.isArray(entries) ? entries : [];
};

export const setMaintenance = (enabled: boolean): Promise<PlatformConfig> => postJson("/api/admin/cabinet/maintenance", { enabled });

export const setReadOnly = (readOnly: boolean): Promise<OperationsMode> => postJson("/api/admin/cabinet/read-only", { read_only: readOnly });

export const setAnnouncement = (body: { title: string; body: string; active: boolean }): Promise<PlatformConfig> => postJson("/api/admin/cabinet/announcement", body);

export const setFeatureFlag = (body: { key: string; description: string; enabled: boolean; rollout: number }): Promise<PlatformConfig> => postJson("/api/admin/cabinet/flag", body);
