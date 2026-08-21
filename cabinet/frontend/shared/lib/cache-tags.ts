// The invalidation vocabulary shared by every cached read and every mutation that moves it.
//
// A tag names a fact about the account, not an endpoint: `wallet` is "what this account
// holds", and a withdrawal, a subscription and a redemption all move it. Naming facts is
// what lets a mutation say what it changed without knowing which screens are mounted —
// `submitWithdrawal` names wallet · withdrawals · operations and every open surface reading
// any of the three refreshes itself.
//
// Kept in one file because the interesting cases cross entity boundaries: subscribing to a
// fund moves the wallet AND the positions AND the timeline. A per-entity constant would put
// the three-way relationship nowhere.

export const TAG = {
  /** Balance and holdings. */
  wallet: "wallet",
  /** The caller's own withdrawal requests. */
  withdrawals: "withdrawals",
  /** The caller's own deposits. */
  deposits: "deposits",
  /** Fund positions — units, cost basis, P&L. */
  positions: "positions",
  /** The caller's own redemption requests. */
  redemptions: "redemptions",
  /** The registry of investable funds. Operator-owned; changes rarely. */
  catalog: "catalog",
  /** Per-fund NAV. Moves only when an operator posts a valuation. */
  nav: "nav",
  /** A fund's fee terms and what the caller's holding has accrued against them. Moves
   *  when the sweeper charges, so it travels with `positions`. */
  fees: "fees",
  /** The merged activity timeline. Every money movement lands here. */
  operations: "operations",
  /** The caller's own profile. */
  profile: "profile",
  /** Notification list and per-topic settings. */
  notifications: "notifications",
  /** The caller's active device sessions. */
  sessions: "sessions",

  // ── operator console ──────────────────────────────────────────────────────────
  /** Fleet health and the parked-event backlog — one screen, two reads that move together. */
  adminFleet: "admin.fleet",
  /** Custody and claim balances across the rails. */
  adminTreasury: "admin.treasury",
  /** Platform config: maintenance, read-only, announcement, flags. */
  adminCabinet: "admin.cabinet",
  /** The withdrawal and redemption queues an operator works through. */
  adminQueue: "admin.queue",
  /** The operator allocation registry — drafts and closed products included. */
  adminAllocations: "admin.allocations",
  /** The user directory and a single user's detail. */
  adminUsers: "admin.users",
  /** The fund's earned revenue and its payouts. A payout also moves `adminQueue` (it
   *  joins the operator withdrawal queue) and `adminTreasury` (it debits a claim), so
   *  the mutation names all three rather than this one alone. */
  adminRevenue: "admin.revenue",
} as const;
