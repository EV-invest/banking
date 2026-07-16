// The single, central catalog of UX tip copy. Editing a tip is a one-line change
// here — the rendering engine (@evinvest/uikit InfoTip / SectionDescriptor) is
// content-agnostic and never sees these strings until <TipAnchor> resolves a key.
//
// Authored as a typed `const` (not JSON) so every anchor id is a compile-time
// literal: a <TipAnchor anchor="…"> referencing a key that does not exist here
// fails `tsc`. Bodies are plain text (no markup) — the bubble is a non-interactive
// help hint, so there are no links to render.
//
// Keys are dot-namespaced by surface (`wallet.*`, `invest.*`, `admin.<area>.*`).
// Investor surfaces are ungated; admin surfaces gate to OPS so operator jargon
// never renders for investors (cosmetic only — server authz stays authoritative).
// Money-safety strings are lifted verbatim from the views so a tip can never drift
// from real behaviour.

export type TipType = "input" | "section";

export interface TipEntry {
  /** Which primitive renders this tip: an inline ⓘ toggletip, or a section block. */
  type: TipType;
  /** Short heading — the toggletip bubble title / the descriptor heading. */
  title: string;
  /** Plain-text explanation, rendered verbatim. */
  body: string;
  /**
   * Optional platform-role gate. When set, only sessions whose role is listed
   * see the tip. Cosmetic only — server-side authorization stays authoritative.
   */
  roles?: readonly string[];
}

export type TipCatalog = Record<string, TipEntry>;

/** Operator-facing surfaces — hidden from investors. */
const OPS = ["operator", "admin", "owner"] as const;

export const tips = {
  // ── wallet ──────────────────────────────────────────────────────────────────
  "wallet.balance.model": {
    type: "section",
    title: "One balance, many rails",
    body: "You have a single USDT balance — networks are just how you deposit and withdraw. This card splits it into Total = Available + Invested + Pending withdrawal.",
  },
  "wallet.balance.available": {
    type: "input",
    title: "Available",
    body: "Your spendable USDT right now — what you can invest into fund units or withdraw.",
  },
  "wallet.balance.invested": {
    type: "input",
    title: "Invested",
    body: "Held in fund units, valued at the current NAV.",
  },
  "wallet.balance.pending-withdrawal": {
    type: "input",
    title: "Pending withdrawal",
    body: "USDT reserved for withdrawals still in flight (queued or processing). It is no longer part of your available balance.",
  },
  "wallet.deposit.network": {
    type: "input",
    title: "Network / rail",
    body: "Pick the chain you'll send USDT on. Each rail shows its own deposit address below — that address only works on that one chain.",
  },
  "wallet.deposit.address": {
    type: "input",
    title: "Your deposit address",
    body: "A personal, reusable address for this rail. On TON it is shown in the wallet-friendly UQ… form so an uninitialised wallet doesn't bounce the transfer.",
  },
  "wallet.deposit.min-confirmations": {
    type: "input",
    title: "Network confirmations",
    body: "Credited to your one balance after the required number of network confirmations.",
  },
  "wallet.deposit.rail-hazard": {
    type: "section",
    title: "Send only USDT on this rail",
    body: "Sending any other asset, or using a different network, loses the funds permanently. This 0x address is NOT your address on the sibling EVM chain — even though both start with 0x, USDT sent on the wrong EVM rail will not be credited.",
  },
  "wallet.withdraw.network": {
    type: "input",
    title: "Network / rail",
    body: "The chain your withdrawal ships on. The destination address must be a valid address on this exact network, or the funds are lost.",
  },
  "wallet.withdraw.destination": {
    type: "input",
    title: "Destination address",
    body: "The on-chain address the USDT is sent to. Withdrawals are irreversible — make sure it is correct and on the network you selected.",
  },
  "wallet.withdraw.available": {
    type: "input",
    title: "Available to withdraw",
    body: "The most you can withdraw on this rail right now. Tap Max to fill the amount.",
  },
  "wallet.withdraw.network-fee": {
    type: "input",
    title: "Network fee",
    body: "A flat on-chain fee for this rail, deducted from your amount before it is sent.",
  },
  "wallet.withdraw.you-receive": {
    type: "input",
    title: "You will receive",
    body: "Your amount minus the network fee — this is what actually lands at the destination.",
  },
  "wallet.withdraw.queueing": {
    type: "section",
    title: "How withdrawals work",
    body: "Up to your instant limit pays out immediately on this rail; anything above it is queued until the rail is topped up. A minimum applies per withdrawal.",
  },
  "wallet.withdraw.review": {
    type: "section",
    title: "Review, then confirm",
    body: "Review freezes exactly what you'll send. Confirm submits that snapshot — if the rail list changes underneath, the open review is voided and you re-review.",
  },

  // ── invest ──────────────────────────────────────────────────────────────────
  "invest.overview": {
    type: "section",
    title: "Fund shares",
    body: "Subscribe USDT for units — your value tracks the fund's NAV, not the unit count.",
  },
  "invest.position.units": {
    type: "input",
    title: "Units",
    body: "Your share count in the fund's pool — a fixed quantity you own until you subscribe or redeem more.",
  },
  "invest.position.nav": {
    type: "input",
    title: "NAV",
    body: "Net asset value per unit — the current price at which units are bought on subscribe and priced on redeem.",
  },
  "invest.position.value": {
    type: "input",
    title: "Value",
    body: "Units × current NAV. It rises and falls as the NAV moves, not with the unit count.",
  },
  "invest.position.stale-nav": {
    type: "input",
    title: "Stale NAV",
    body: "The fund's price feed is out of date, so the value and P&L shown here may lag the true mark until the NAV refreshes.",
  },
  "invest.position.pnl": {
    type: "input",
    title: "Profit & loss",
    body: "Your position's current value minus what you paid in — green for a gain, red for a loss, driven by the NAV moving.",
  },
  "invest.subscribe.pricing": {
    type: "section",
    title: "How subscribing works",
    body: "Units are priced at the current NAV. Profit comes from the NAV rising, not from extra units.",
  },
  "invest.subscribe.fund": {
    type: "input",
    title: "Fund",
    body: "The fund your USDT buys units in. It defaults to the demo fund and rarely needs changing.",
  },
  "invest.subscribe.amount": {
    type: "input",
    title: "Amount",
    body: "How much USDT to subscribe. It is converted into units at the current NAV when you submit.",
  },
  "invest.redeem.units": {
    type: "input",
    title: "Units to redeem",
    body: "How many units to redeem — Max fills your full holding. The resulting cash isn't fixed here; it is set at the settle-time NAV.",
  },
  "invest.redeem.queue": {
    type: "section",
    title: "How redemptions settle",
    body: "Redemptions are accept-and-queue — your units are reserved now, and cash is priced at the settle-time NAV once the fund tops up.",
  },
  "invest.activity.status": {
    type: "input",
    title: "Redemption status",
    body: "Queued (reserved, awaiting settle), completed (paid out at the settle NAV), failed, or cancelled.",
  },
  "invest.activity.cancel": {
    type: "input",
    title: "Cancel",
    body: "Available only while a redemption is still queued. It withdraws the request and releases the reserved units back to your holding before any settle NAV is applied.",
  },

  // ── dashboard ───────────────────────────────────────────────────────────────
  "dashboard.performance.portfolio-value": {
    type: "input",
    title: "Portfolio value",
    body: "Your whole balance: available + invested + pending withdrawal. Money locked in a queued withdrawal is still counted here.",
  },
  "dashboard.performance.all-time-return": {
    type: "input",
    title: "All-time return",
    body: "Total unrealised P&L divided by what you put in at cost basis, across every position. A paper figure, not realised cash.",
  },
  "dashboard.performance.series": {
    type: "section",
    title: "Performance chart",
    body: "'Fund performance' is the fund's own return; 'Your participation' is how your stake performed, which differs by when you contributed. The live series isn't wired up yet.",
  },
  "dashboard.move-money.auto-deploy": {
    type: "input",
    title: "Auto-deploy idle cash",
    body: "When on, your available cash is automatically committed into strategies at end of day.",
  },
  "dashboard.invested.allocation": {
    type: "input",
    title: "Invested — what I own",
    body: "Each bar is one strategy's share of your invested value at current NAV. Percentages are of invested value only and exclude your available cash.",
  },
  "dashboard.stats.unrealized-pnl": {
    type: "input",
    title: "Unrealised P&L",
    body: "Paper gain or loss across all positions — current value minus cost basis. It becomes realised cash only when you redeem.",
  },
  "dashboard.stats.available": {
    type: "input",
    title: "Available",
    body: "Cash that is free and spendable now — ready to deploy into a strategy or withdraw.",
  },
  "dashboard.stats.net-invested": {
    type: "input",
    title: "Net invested",
    body: "Total cash you put into strategies at cost basis, net of redemptions — not the current value.",
  },

  // ── settings ────────────────────────────────────────────────────────────────
  "settings.security.google-signin": {
    type: "section",
    title: "Sign-in is managed by Google",
    body: "Your sign-in and password are managed by Google. Two-factor authentication and recovery are configured in your Google Account.",
  },
  "settings.sessions.overview": {
    type: "section",
    title: "Sessions & devices",
    body: "Where you're signed in — each row is a device or browser with a live session. Revoke anything you don't recognise.",
  },
  "settings.sessions.this-device": {
    type: "input",
    title: "This device",
    body: "The browser you're using right now. It stays signed in and can't be revoked from here — use 'Sign out all other devices' to clear the rest.",
  },
  "settings.sessions.revoke": {
    type: "input",
    title: "Revoke",
    body: "Signs that device out immediately. It must sign in again with Google to regain access.",
  },
  "settings.sessions.revoke-others": {
    type: "input",
    title: "Sign out all other devices",
    body: "Signs out every device except this one. Use it if a device was lost or you don't recognise a session.",
  },

  // ── profile ─────────────────────────────────────────────────────────────────
  "profile.personal.compliance": {
    type: "section",
    title: "Why we collect this",
    body: "These personal details are collected to meet the fund's compliance obligations and to produce your account statements. They are not shown publicly.",
  },
  "profile.field.legal-name": {
    type: "input",
    title: "Legal name",
    body: "Your full name exactly as it appears on official documents — used for compliance and statements, distinct from your preferred name.",
  },
  "profile.field.nationality": {
    type: "input",
    title: "Nationality",
    body: "Collected for compliance and eligibility checks.",
  },
  "profile.field.tax-residence": {
    type: "input",
    title: "Tax residence",
    body: "The country where you're liable to pay tax, used for tax reporting. Usually where you live — it can differ from your nationality.",
  },
  "profile.email.verified": {
    type: "input",
    title: "Verified",
    body: "Confirms this email address has been verified. It is not identity or KYC verification.",
  },

  // ── admin · users ───────────────────────────────────────────────────────────
  "admin.users.access.role": {
    type: "input",
    title: "Role",
    body: "The access level for this user: investor, operator, admin, or owner. Raising it grants operator/admin console access immediately on change.",
    roles: OPS,
  },
  "admin.users.access.kyc-level": {
    type: "input",
    title: "KYC level",
    // Per-level gating is defined by the KYC/money-plane policy, not in the view —
    // confirm the exact tier→limit mapping with policy before shipping.
    body: "The user's identity-verification tier. Higher levels unlock higher limits and actions per the KYC policy.",
    roles: OPS,
  },
  "admin.users.access.revoke-sessions": {
    type: "input",
    title: "Revoke all sessions",
    body: "Bumps token_version — invalidates every JWT issued to this user, signing them out of every active session.",
    roles: OPS,
  },
  "admin.users.identity.token-version": {
    type: "input",
    title: "Token version",
    body: "A per-user counter stamped into every issued JWT. It increments on 'Revoke all sessions', so any token carrying an older version fails verification.",
    roles: OPS,
  },
  "admin.users.status.suspend": {
    type: "input",
    title: "Suspend / Reinstate",
    body: "Suspend disables the account and blocks the user's access; Reinstate re-enables it. Reversible — the button reflects the current status.",
    roles: OPS,
  },

  // ── admin · overview ────────────────────────────────────────────────────────
  "admin.overview.kpi.parked-rows": {
    type: "input",
    title: "Parked rows",
    body: "Money the relay couldn't apply — outbox rows stuck until unparked. Nonzero turns the tile red.",
    roles: OPS,
  },
  "admin.overview.kpi.dispatch-backlog": {
    type: "input",
    title: "Dispatch backlog",
    body: "Undispatched outbox rows — events written but not yet relayed. A healthy relay keeps this near zero.",
    roles: OPS,
  },
  "admin.overview.kpi.oldest-backlog": {
    type: "input",
    title: "Oldest backlog",
    body: "Age of the oldest undispatched row. A growing age means the relay is stalled or falling behind.",
    roles: OPS,
  },
  "admin.overview.kpi.dead-key-signings": {
    type: "input",
    title: "Dead-key signings",
    body: "The signer couldn't unseal a wallet key (wrong or rotated KEK epoch), so those funds can't be moved. Nonzero is a money-safety alarm — page an operator.",
    roles: OPS,
  },
  "admin.overview.parked-events": {
    type: "section",
    title: "Parked events",
    body: "A row parks when the relay hits a terminal apply error. Fix the cause shown in the Reason column first, then unpark to re-drive it — otherwise it just re-parks.",
    roles: OPS,
  },
  "admin.overview.parked.reason": {
    type: "input",
    title: "Reason",
    body: "The relay's failure cause for this parked row — the thing you must fix before unparking.",
    roles: OPS,
  },
  "admin.overview.parked.compensated": {
    type: "input",
    title: "Compensated",
    body: "This row's saga was already reversed, so its money effect is undone. Unpark is disabled to avoid re-applying an entry that was intentionally rolled back.",
    roles: OPS,
  },
  "admin.overview.parked.unpark": {
    type: "input",
    title: "Unpark",
    body: "Re-drives this outbox row through the relay after you've fixed its cause. Disabled when the row was compensated, already unparked this session, or while another unpark is in flight.",
    roles: OPS,
  },

  // ── admin · treasury ────────────────────────────────────────────────────────
  "admin.treasury.two-layer-model": {
    type: "section",
    title: "Two layers",
    body: "Layer 1 is the ledger's network-agnostic USDT claims; Layer 2 is the actual on-chain liquidity held per rail. The two must reconcile.",
    roles: OPS,
  },
  "admin.treasury.layer1.ledger": {
    type: "section",
    title: "Layer 1 · Ledger",
    body: "The ledger's network-agnostic claims in USDT. Total claims equal on-chain custody and break down into held-for-clients, fund capital, and reserved-for-withdrawals.",
    roles: OPS,
  },
  "admin.treasury.layer1.claims-total": {
    type: "input",
    title: "Claims · total",
    body: "Total user + fund claims. Equals the sum of on-chain custody — every claim is fully backed.",
    roles: OPS,
  },
  "admin.treasury.layer1.held-for-clients": {
    type: "input",
    title: "Held for clients",
    body: "The portion of claims owed to clients — user wallet plus service balances.",
    roles: OPS,
  },
  "admin.treasury.layer1.fund-capital": {
    type: "input",
    title: "Fund capital",
    body: "Claims that are the fund's own capital, not client money.",
    roles: OPS,
  },
  "admin.treasury.layer1.reserved-withdrawals": {
    type: "input",
    title: "Reserved · withdrawals",
    body: "Claims set aside for queued and in-flight withdrawals, parked in the clearing account until they settle on-chain.",
    roles: OPS,
  },
  "admin.treasury.layer2.rails": {
    type: "section",
    title: "Layer 2 · Treasury",
    body: "The actual on-chain USDT liquidity held per rail (BEP20 / TRC20 / TON / Polygon), plus the fiat bank balance. This is where the backing physically sits.",
    roles: OPS,
  },
  "admin.treasury.rail.funding": {
    type: "section",
    title: "Rail funding",
    body: "The card value is this rail's ledger custody; the footer shows the hot wallet's real on-chain USDT and native gas. The two can legitimately diverge under accept-and-queue.",
    roles: OPS,
  },
  "admin.treasury.rail.address": {
    type: "input",
    title: "Treasury address",
    body: "This rail's on-chain hot-wallet address. BEP20 and Polygon share an identical EVM address format — topping up on the wrong EVM chain, or sending another rail's funds here, is an irreversible mis-send.",
    roles: OPS,
  },
  "admin.treasury.rail.gas-station": {
    type: "input",
    title: "Gas station",
    body: "Top this address up with the rail's native token to fund sweep gas. Send the correct native symbol on the correct chain — EVM gas-station addresses are shared, so a wrong-chain top-up is unrecoverable.",
    roles: OPS,
  },
  "admin.treasury.bank": {
    type: "input",
    title: "Bank · USD",
    body: "The fiat USD bank balance used for off-ramp and FX, counted as treasury liquidity alongside the on-chain rails.",
    roles: OPS,
  },
  "admin.treasury.invariant": {
    type: "section",
    title: "The invariant",
    body: "Per-rail backing is the treasury's job, not the ledger's: a shortfall on one rail is accept-and-queue, then rebalanced via CEX, alt-rail, or top-up. The global invariant is sum(custody) == sum(claims).",
    roles: OPS,
  },

  // ── admin · valuation ───────────────────────────────────────────────────────
  "admin.valuation.post.aum": {
    type: "input",
    title: "AUM (USDT)",
    body: "The fund's total assets under management you're marking. The NAV/share that pays redemptions is derived from it (AUM ÷ units outstanding), so a wrong figure mis-prices every settle.",
    roles: OPS,
  },
  "admin.valuation.post.derived-nav": {
    type: "input",
    title: "Derived NAV / share",
    body: "Entered AUM divided by units outstanding. This is the mark that Post valuation commits.",
    roles: OPS,
  },
  "admin.valuation.post.nav-guard": {
    type: "section",
    title: "NAV-move guard",
    body: "A post is rejected if the NAV moves more than 50% from the last mark, unless Override guard is on.",
    roles: OPS,
  },
  "admin.valuation.post.override": {
    type: "input",
    title: "Override guard",
    body: "Bypasses the NAV-move guard so a post that moves NAV more than 50% is accepted and settles redemptions at that mark. A fat-fingered AUM then over- or under-pays irreversibly.",
    roles: OPS,
  },
  "admin.valuation.queue.settle-fail": {
    type: "section",
    title: "Settle vs Fail",
    body: "Settle pays at settle-time NAV once the fund claim is liquid; if the rail is short the payout queues until treasury tops up. Fail voids the request and refunds the units.",
    roles: OPS,
  },
  "admin.valuation.queue.est-cash": {
    type: "input",
    title: "Est. cash",
    body: "A preview only — approximately units × current NAV. The actual payout settles at the settle-time NAV, not this figure.",
    roles: OPS,
  },
  "admin.valuation.queue.settle": {
    type: "input",
    title: "Settle",
    body: "Pays at the settle-time NAV once the fund claim is liquid, queuing if the rail is short. Settle also burns the user's units.",
    roles: OPS,
  },
  "admin.valuation.queue.fail": {
    type: "input",
    title: "Fail",
    body: "Voids the request and refunds the units — no cash is paid and the burned units are returned to the user.",
    roles: OPS,
  },

  // ── admin · withdrawals ─────────────────────────────────────────────────────
  "admin.withdrawals.flow": {
    type: "section",
    title: "Dispatch, settle, fail",
    body: "Dispatch broadcasts a queued withdrawal once its rail has liquidity. Settle records the mined transaction and releases the reservation. Fail voids and refunds — only safe when nothing reached the chain.",
    roles: OPS,
  },
  "admin.withdrawals.gross-net": {
    type: "input",
    title: "Gross / net",
    body: "Gross is the full amount debited from the user; net is what is actually sent on-chain after the withdrawal fee.",
    roles: OPS,
  },
  "admin.withdrawals.state": {
    type: "input",
    title: "State",
    body: "Queued = accepted and reserved but not yet broadcast (Dispatch only). Processing = already broadcast, awaiting a mined tx (Settle or Fail apply).",
    roles: OPS,
  },
  "admin.withdrawals.settle.tx-hash": {
    type: "input",
    title: "Mined transaction hash",
    body: "Paste the mined on-chain tx hash. Settling records the mined transaction and releases the reservation — only enter a hash for a tx that actually mined.",
    roles: OPS,
  },
  "admin.withdrawals.fail.double-pay": {
    type: "section",
    title: "Failing can double-pay",
    body: "Failing refunds the user. If the broadcast reached the chain this would double-pay — the hub refuses while a broadcast record exists, but verify on-chain first.",
    roles: OPS,
  },
  "admin.withdrawals.destination": {
    type: "input",
    title: "Destination",
    body: "The user's payout address, bound to this specific rail. Sends are irreversible and rail-specific — verify the address matches the network before dispatching.",
    roles: OPS,
  },

  // ── admin · cabinet ─────────────────────────────────────────────────────────
  "admin.cabinet.flags": {
    type: "section",
    title: "Feature flags",
    body: "Flags gate cabinet features and MFE mounts. The row toggle flips a flag on or off; the rollout % controls what share of users it is exposed to.",
    roles: OPS,
  },
  "admin.cabinet.flags.rollout": {
    type: "input",
    title: "Rollout %",
    body: "The share of users the flag is enabled for during a staged rollout. 100% is a full rollout; lower values expose it to only that fraction.",
    roles: OPS,
  },
  "admin.cabinet.announcement.live": {
    type: "input",
    title: "Live",
    body: "Publishes the announcement banner across the whole cabinet immediately.",
    roles: OPS,
  },
  "admin.cabinet.maintenance": {
    type: "input",
    title: "Maintenance mode",
    body: "Swaps the whole cabinet for a holding page for all users — an identity-plane kill-switch. No money movement is affected.",
    roles: OPS,
  },
  "admin.cabinet.readonly": {
    type: "input",
    title: "Read-only mode",
    body: "Halts all deposit and withdrawal money movement — the money-plane kill-switch.",
    roles: OPS,
  },
} as const satisfies TipCatalog;

export type TipKey = keyof typeof tips;
