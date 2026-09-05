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

// ── fees ─────────────────────────────────────────────────────────────────────

/** A fund's terms. `configured` false means no policy row exists, which is a different
 *  fact from a policy whose rates are zero — the first can never charge. */
export interface FeePolicy {
  service: string;
  configured: boolean;
  management_bps: number;
  performance_bps: number;
  hurdle_bps: number;
  basis: string;
  crystallization: string;
  updated_at: string;
}

export interface FeePolicyList {
  policies: FeePolicy[];
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
}

export interface AllocationList {
  allocations: Allocation[];
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
