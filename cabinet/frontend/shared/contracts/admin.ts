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
}

// ── users ─────────────────────────────────────────────────────────────────────
export interface AdminUserSummary {
  user_id: string;
  email: string;
  status: string;
  kyc_level: number;
  role: string;
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
}

export interface UserBalance {
  amount: string;
  pending: string;
  authoritative: boolean;
  as_of: string;
}

// ── treasury ──────────────────────────────────────────────────────────────────
export interface RailLiquidity {
  network: string;
  custody: string;
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

// ── valuation + redemptions ─────────────────────────────────────────────────────
export interface FundNav {
  service: string;
  nav: string;
  aum: string;
  units_outstanding: string;
  posted_at: string;
  stale: boolean;
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
