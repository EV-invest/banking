// Admin-console response types — the shapes the BFF's `/api/admin/*` handlers emit
// (see `cabinet/backend/src/dto.rs`). These are hand-written to match the BFF DTOs
// rather than re-exported from `./gen`, because the admin DTOs diverge from the raw
// proto (64-bit ints rendered as strings, the derived `isAdmin` flag, and the combined
// cabinet-config response). Import these from `@/shared/contracts/admin`.

export interface SessionUser {
  userId: string;
  email: string;
  status: string;
  role: string;
  isAdmin: boolean;
  /**
   * `role` above came from the `OWNER_SUBJECTS` emergency allowlist, not from the
   * persisted `users.role`.
   *
   * camelCase, unlike its snake_case twin on {@link AdminUserSummary} below, and the
   * difference is not a typo. This shape is NOT one of the BFF's: `/api/auth/session` is
   * the site-root auth surface, and ingress sends `/api/auth/*` straight to concierge,
   * whose `SessionUser` carries `#[serde(rename_all = "camelCase")]`. The admin DTOs come
   * from the cabinet's own BFF (`cabinet/backend/src/dto.rs`), which renames nothing.
   *
   * True only while the fund has no persisted owner — the window closes on the first
   * genesis seeding and cannot reopen. Render it as a warning, never as ownership: it is
   * real authority over the console and it seats nobody.
   */
  roleIsBreakGlass: boolean;
}

export interface SessionInfo {
  authenticated: boolean;
  user?: SessionUser;
}

// ── overview ──────────────────────────────────────────────────────────────────
export interface FleetService {
  name: string;
  kind: string;
  status: string;
  detail: string;
}

export interface AdminOverview {
  services: FleetService[];
  parked_rows: string;
  backlog: string;
  oldest_backlog_age_secs: string;
  /** Signer unseal failures since hub boot — non-zero means a dead key was asked to sign (funds stranded). */
  unseal_failures: string;
}

// ── users ─────────────────────────────────────────────────────────────────────
export interface AdminUserSummary {
  user_id: string;
  email: string;
  status: string;
  kyc_level: number;
  role: string;
  /** @see SessionUser.roleIsBreakGlass — same flag, the BFF's snake_case spelling. */
  role_is_break_glass: boolean;
  token_version: string;
  created_at: string;
  /**
   * WHY the account is disabled — and therefore which control the console may offer.
   * THREE cases, not two:
   *
   *   · `"admin_hold"`  — one operator's brake. Lapses by itself at {@link hold_expires_at},
   *                       and `/api/admin/users/reinstate` lifts it in one act.
   *   · `"governance"`  — the owners' ratified verdict. Never lapses; reinstatement is a
   *                       proposal, and the one-act route refuses it.
   *   · `""`            — empty on an ACTIVE user, and also on one suspended before this
   *                       field existed. Those keep the old one-act, never-lapsing
   *                       semantics they were actually suspended under, so an empty string
   *                       is not a synonym for "active": read it together with `status`.
   *
   * Which is why this is a string and not a boolean. A two-way branch files the third case
   * under whichever of the other two it happened to be compared against, and the account it
   * mis-files is the one nobody can act on correctly.
   */
  suspended_by: string;
  /** Unix seconds an `admin_hold` lapses; `"0"` when nothing lapses. Format through
   *  `shared/lib/datetime`, which is where this wire shape (seconds-as-string, `"0"` for
   *  absent) is already understood. */
  hold_expires_at: string;
}

export interface AdminUserList {
  users: AdminUserSummary[];
  total: string;
}

export interface AdminUserProfile {
  user_id: string;
  email: string;
  email_verified: boolean;
  status: string;
  token_version: string;
  legal_name: string;
  preferred_name: string;
  phone: string;
  date_of_birth: string;
  nationality: string;
  tax_residence: string;
  residential_address: string;
  language: string;
  base_currency: string;
  timezone: string;
  kyc_level: number;
  role: string;
  /** @see SessionUser.roleIsBreakGlass — same flag, the BFF's snake_case spelling. */
  role_is_break_glass: boolean;
  /**
   * WHY the account is disabled — and therefore which control the console may offer.
   * THREE cases, not two:
   *
   *   · `"admin_hold"`  — one operator's brake. Lapses by itself at {@link hold_expires_at},
   *                       and `/api/admin/users/reinstate` lifts it in one act.
   *   · `"governance"`  — the owners' ratified verdict. Never lapses; reinstatement is a
   *                       proposal, and the one-act route refuses it.
   *   · `""`            — empty on an ACTIVE user, and also on one suspended before this
   *                       field existed. Those keep the old one-act, never-lapsing
   *                       semantics they were actually suspended under, so an empty string
   *                       is not a synonym for "active": read it together with `status`.
   *
   * Which is why this is a string and not a boolean. A two-way branch files the third case
   * under whichever of the other two it happened to be compared against, and the account it
   * mis-files is the one nobody can act on correctly.
   */
  suspended_by: string;
  /** Unix seconds an `admin_hold` lapses; `"0"` when nothing lapses. Format through
   *  `shared/lib/datetime`, which is where this wire shape (seconds-as-string, `"0"` for
   *  absent) is already understood. */
  hold_expires_at: string;
}

export interface UserBalance {
  amount: string;
  pending: string;
  authoritative: boolean;
  as_of: string;
}

// ── outbox ────────────────────────────────────────────────────────────────────
export interface ParkedEvent {
  seq: string;
  event_id: string;
  aggregate: string;
  aggregate_id: string;
  kind: string;
  reason: string;
  parked_at: string;
  compensated: boolean;
}

export interface ParkedEventList {
  events: ParkedEvent[];
}

// ── treasury ──────────────────────────────────────────────────────────────────
export interface RailLiquidity {
  network: string;
  custody: string;
  treasury_address: string;
  onchain_usdt: string;
  onchain_gas: string;
  /** The rail's sweep gas-station wallet — fund native coin here (never USDT). Empty when unwired. */
  gas_station_address: string;
  gas_station_gas: string;
  /** Whether the rail's addresses are testnet-tagged — TON's friendly form differs per realm. */
  is_testnet: boolean;
}

export interface Treasury {
  rails: RailLiquidity[];
  bank: string;
  total_custody: string;
  fund_capital: string;
  fee_revenue: string;
  held_for_clients: string;
  reserved_for_withdrawals: string;
}

// ── allocations (the registry of investable products) ───────────────────────────

/** `draft` — registered, takes no money yet. `open` — subscribe + redeem. `closed` —
 *  redeem only, so winding a product down never traps an investor. */
export type AllocationState = "draft" | "open" | "closed";

/** The picture an operator chose for a product, in the wire's snake_case.
 *
 *  A closed set mirrored from `icon::ALL` in `contracts/src/allocation.rs`; the hub's CHECK
 *  constraint and the BFF both reject anything outside it (the BFF answers 400), so this
 *  union and that list have to be edited together. `fund` is the hub's default. */
export type AllocationIcon = "fund" | "real_estate" | "trading" | "yield" | "venture" | "treasury" | "commodity" | "credit" | "index" | "arbitrage";

/** `hidden` — not in the caller's catalog at all. `view` — listed, `Subscribe` refused.
 *  `invest` — listed and open to new money (subject to `state` and the unit cap). Ranked
 *  `hidden < view < invest`, mirroring `contracts::allocation::access::ALL`. */
export type AllocationAccessLevel = "hidden" | "view" | "invest";

/** The levels a per-investor grant may carry — everything above the `hidden` floor.
 *  Mirrors `contracts::allocation::access::GRANTABLE`. */
export type AllocationGrantLevel = "view" | "invest";

// ── fees ─────────────────────────────────────────────────────────────────────

/** The five-field schedule a fund charges by — one statement, never patched a leg at a
 *  time. Rates are basis points; `basis` and `crystallization` are the money plane's closed
 *  vocabularies (`shared/lib/fee-terms.ts` has the words for them). */
export interface FeeTerms {
  management_bps: number;
  performance_bps: number;
  hurdle_bps: number;
  basis: string;
  crystallization: string;
}

/** A fund's terms in force. `configured` false means no policy row exists, which is a
 *  different fact from a policy whose rates are zero — the first can never charge. */
export interface FeePolicy extends FeeTerms {
  service: string;
  configured: boolean;
  /** Unix seconds as a string; the moment the row was last promoted. */
  updated_at: string;
  /** Which version of the fund's history these terms are; `0` when unconfigured. */
  version: number;
  /** Unix seconds as a string since which they bind; `"0"` when unconfigured. */
  effective_from: string;
  /** The change on its way — awaiting the owners or scheduled — or absent. Shown to
   *  holders and non-holders alike: the terms coming are part of deciding to stay in. */
  pending?: FeePolicyChange | null;
}

export interface FeePolicyList {
  policies: FeePolicy[];
}

/** Where a change stands. Typed as an open string like every other lifecycle on this wire
 *  (`./governance`): a member the plane grows later must render as its wire word, not
 *  break the build. The names this build knows are `admin.feeChange.state.*`. */
export type FeePolicyChangeState = "awaiting_consilium" | "scheduled" | "active" | "superseded" | "rejected" | "cancelled" | string;

/** Who had to agree: one `AllocationManage` holder, or the owners' consilium. */
export type FeePolicyRequirement = "admin" | "owner_consilium" | string;

/**
 * One row of a fund's fee-policy history — what `POST /api/admin/fees/policy` and
 * `/policy/cancel` answer with, and the rows of `GET /api/admin/fees/changes`.
 *
 * Timestamps are unix seconds as strings, `"0"` where the moment has not come:
 * `effective_from` is provisional while the change awaits the owners (the 24h notice is
 * counted from their approval, not from the request) and fixed once scheduled.
 */
export interface FeePolicyChange extends FeeTerms {
  id: string;
  service: string;
  version: number;
  state: FeePolicyChangeState;
  effective_from: string;
  requirement: FeePolicyRequirement;
  /** The consilium this change waits on; `null` for an administrator's change. */
  consilium_id?: string | null;
  requested_by: string;
  requested_at: string;
  scheduled_at: string;
  applied_at: string;
  reason: string;
}

export interface FeePolicyChangeList {
  changes: FeePolicyChange[];
}

/** `POST /api/admin/fees/policy`. `effective_from` is unix seconds as a NUMBER — the BFF
 *  reads it with `as_i64` — and `0` asks for the earliest moment the notice allows.
 *  `reason` is required by the hub exactly when the owners must approve. */
export interface ScheduleFeePolicyRequest extends FeeTerms {
  service: string;
  effective_from: number;
  reason: string;
}

/** `POST /api/admin/fees/policy/cancel`. Idempotent on an already-cancelled change. */
export interface CancelFeePolicyChangeRequest {
  service: string;
  change_id: string;
}

/** Uncollected fee units in one fund. `value` is what a settlement would convert them to
 *  at the current NAV. */
export interface FeeShares {
  service: string;
  units: string;
  value: string;
}

export interface FeeSettlement {
  service: string;
  units: string;
  nav: string;
  cash: string;
}

/** One charge against one holding. `charged_cash` falls short of
 *  `management + performance` exactly when the rest went to `debt_carried`. */
export interface FeeAssessment {
  service: string;
  trigger: string;
  nav: string;
  management: string;
  performance: string;
  debt_opening: string;
  charged_units: string;
  charged_cash: string;
  debt_carried: string;
  high_water_mark: string;
  assessed_at: string;
}

export interface FeeAssessmentList {
  assessments: FeeAssessment[];
}

export interface Allocation {
  service: string;
  title: string;
  summary: string;
  state: AllocationState;
  /** Unix seconds; `"0"` on the write responses, which return the clock-free aggregate. */
  created_at: string;
  updated_at: string;
  /** Authorised unit supply, decimal. Subscribe refuses a mint that would pass it. */
  unit_cap: string;
  /**
   * Optional on READ, and deliberately so rather than as a hedge: `allocationsResource`
   * carries `persist: true`, so a returning user's first frame is rehydrated from a
   * sessionStorage object serialised before this field existed. Optionality alone is not
   * the fix — render it through `ProductIcon` (`@/shared/ui/icons/products`), which lands
   * both a missing value and one newer than this build on `fund`.
   *
   * Required on WRITE by this client's own choice, not by the hub — see
   * `AllocationWrite`. On the wire the field carries presence, so an update that omits
   * it changes nothing.
   */
  icon?: AllocationIcon;
  /**
   * access
   *
   * The product's default access level — what a caller with no grant holds. A
   * registration lands on `view`: listed, locked. Optional on READ for the same reason
   * `icon` is above — `allocationsResource` persists to sessionStorage, and a returning
   * user's first frame may rehydrate an object serialised before this field existed.
   */
  access?: AllocationAccessLevel;
  /**
   * caller_access
   *
   * The level THIS caller effectively holds — `max(access, their own grant)`. The
   * subscribe control and the "locked" badge key off this, never off `access` or `state`
   * alone: an `open` product at `view` is visible and not investable. Honest for an
   * `AllocationManage` holder too — their permission does not make them an investor. Same
   * optionality caveat as `access`.
   */
  caller_access?: AllocationAccessLevel;
}

export interface AllocationList {
  allocations: Allocation[];
}

/** One investor raised above a product's default access level. */
export interface AllocationAccessGrant {
  service: string;
  /**
   * The investor — the id the console carries: concierge-first, banking when no mirror
   * exists. Revoke sends it back exactly as received; the BFF owns the mapping.
   */
  user_id: string;
  /**
   * The investor's email, for display. `null` when the hub could not match `user_id` to
   * an identity (a banking-only id with no concierge mirror, or a mirror that has since
   * gone); absent on a payload serialised before the field existed. Fall back to the id.
   */
  email?: string | null;
  level: AllocationGrantLevel;
  /** The `AllocationManage` holder who granted it — console-facing id, same rule as `user_id`. */
  granted_by: string;
  /** Unix seconds. */
  granted_at: string;
}

export interface AllocationAccessGrantList {
  grants: AllocationAccessGrant[];
}

// ── in-kind issuance (units with no cash leg) ───────────────────────────────────

/** Who an in-kind mint lands on: one investor, or the fund's own stake. */
export type UnitHolderKind = "user" | "company";

/** `queued` until the relay posts the mint, then `applied`. A `queued` row is real — the
 *  hub has accepted it — but the units are not on the ledger yet, so a holders read
 *  taken straight after the POST still shows the supply as it was. */
export type UnitIssuanceState = "queued" | "applied";

/** Where the units came from: `mint` (`/allocations/issue` — the supply grew by `units`)
 *  or `company` (`/allocations/transfer-stake` — moved out of the company's stake, the
 *  supply unchanged). Always populated; a row that predates the field reads as `mint`. */
export type UnitIssuanceSource = "mint" | "company";

/** One in-kind issuance — a mint or a hand-over of the company's stake — as the hub
 *  recorded it. */
export interface UnitIssuance {
  id: string;
  service: string;
  holder_kind: UnitHolderKind;
  /** The banking user id for a `user` holder; empty for `company`. */
  holder_id: string;
  units: string;
  /** Decimal USDT per unit the mint was recorded at. */
  nav: string;
  /** Decimal USDT the holder is deemed to have paid — what P&L and the management fee
   *  are measured from. */
  cost_basis: string;
  state: UnitIssuanceState;
  /** Unix seconds. */
  created_at: string;
  source: UnitIssuanceSource;
}

/** The hand-over body, exactly as the BFF reads it (`POST /api/admin/allocations/
 *  transfer-stake`). Always a user — the company handing units to itself is not a
 *  request. `cost_basis` present only when the operator typed one: absent means
 *  `units × NAV` hub-side, and an empty string is NOT the same as absent.
 *  `views/admin/allocations/lib/transfer-stake.ts` is the one place that builds it. */
export interface TransferStakeBody {
  service: string;
  /** The recipient — the id the console carries. */
  user_id: string;
  /** Decimal units, > 0, at most what the company holds. */
  units: string;
  /** Decimal USDT the recipient is deemed to have paid; omitted = `units × NAV`. */
  cost_basis?: string;
  /** 1..64 chars, in the same per-product key space as `/allocations/issue`. The same
   *  retry contract: one key per submission, the same key on a retry of it. */
  idempotency_key: string;
}

/** A product's settled supply by holder class, all decimal units. `investor_units` is
 *  `units_outstanding − company_units − fee_units`; the supply invariant makes the
 *  difference exact. */
export interface UnitHolders {
  service: string;
  units_outstanding: string;
  company_units: string;
  fee_units: string;
  investor_units: string;
}

// ── valuation + redemptions ─────────────────────────────────────────────────────
export interface FundNav {
  service: string;
  nav: string;
  aum: string;
  /** The settled supply — the denominator NAV is derived against. */
  units_outstanding: string;
  posted_at: string;
  stale: boolean;
  unit_cap: string;
  /** Units still issuable. Already nets off in-flight mints, so offering this figure can
   *  never offer more than the hub will accept. */
  remaining_capacity: string;
  /** Of `units_outstanding`, the company's own in-kind stake — issued through
   *  `/allocations/issue`, never bought through Subscribe. Optional on READ for the same
   *  reason `Allocation.icon` is: `fundNavResource` persists to sessionStorage, and a
   *  returning user's first frame may rehydrate a mark serialised before this field
   *  existed. */
  company_units?: string;
}

export interface RedemptionQueueItem {
  redemption_id: string;
  user_id: string;
  email: string;
  service: string;
  units: string;
  created_at: string;
}

export interface RedemptionQueue {
  items: RedemptionQueueItem[];
}

export interface Redemption {
  id: string;
  service: string;
  units: string;
  nav: string;
  cash: string;
  state: string;
}

// ── withdrawals (operator queue) ─────────────────────────────────────────────────
export interface WithdrawalQueueItem {
  withdrawal_id: string;
  /** Which claim funds it. A `revenue` row is the fund paying its own earnings out, so
   *  it carries no `user_id`/`email` — label it rather than rendering a blank investor. */
  source: "user" | "revenue";
  user_id: string;
  email: string;
  network: string;
  address: string;
  amount: string;
  net_amount: string;
  state: string;
  created_at: string;
}

export interface WithdrawalQueue {
  items: WithdrawalQueueItem[];
}

// ── revenue (the fund's own earned money) ──────────────────────────────────────

/** Per-rail payout options. `payable` is the whole available revenue (a request beyond
 *  `instant` is accepted and queued until the treasury is topped up); `instant` ships now. */
export interface RevenueRail {
  network: string;
  payable: string;
  instant: string;
  minimum: string;
}

/** What the fund EARNED and may pay itself — the `fee` claim, credited by the fee
 *  retained on a user withdrawal and by the settled 2-and-20. Client balances and the
 *  fund's seed capital are separate claims and are not in this figure.
 *  `earned = available + pending_payout`, all three off one ledger balance. */
export interface FundRevenue {
  earned: string;
  available: string;
  pending_payout: string;
  rails: RevenueRail[];
}

/** A payout, shaped exactly like a user withdrawal — same saga, same states. `fee` is
 *  always `"0"`: the fee claim is where fees are retained, so a payout charges none. */
export interface RevenuePayout {
  id: string;
  network: string;
  address: string;
  amount: string;
  fee: string;
  net_amount: string;
  state: string;
  tx_ref: string;
}

export interface RevenuePayoutList {
  withdrawals: RevenuePayout[];
}

// ── cabinet (platform config + money-plane read-only) ───────────────────────────
export interface FeatureFlag {
  key: string;
  description: string;
  enabled: boolean;
  rollout: number;
}

export interface PlatformConfig {
  maintenance_mode: boolean;
  announcement_title: string;
  announcement_body: string;
  announcement_active: boolean;
  flags: FeatureFlag[];
}

export interface OperationsMode {
  read_only: boolean;
}

export interface CabinetConfig {
  platform: PlatformConfig;
  read_only: boolean;
}
