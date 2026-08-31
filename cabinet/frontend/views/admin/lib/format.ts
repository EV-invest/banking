// Formatting + small helpers shared by the admin-console views. Money comes from the
// cabinet's one money module (`@/shared/lib/money`) — the console reads the same figures
// an investor does, so it shows them at the same precision.
//
// Nothing here holds a translator of its own: this is a plain module, so every helper that
// produces words takes the caller's `t`. The views are all client components, so `t` is
// one `useT()` away at every call site — and keeping it a parameter is what stops a second
// copy of the catalogue (or React) leaking into a module that only formats.

import type { Translate } from "@evinvest/i18n";

/** `formatAmount` is a decimal amount → grouped display with no currency symbol; the
 *  admin tables spell the unit out in the header instead. */
export { compactUnits, formatAmount as amount, formatNav, formatUnits, formatUsd, fractionOfCap, toBaseUnits } from "@/shared/lib/money";

/** A unix-seconds string → a coarse "3h ago" age (for queue/session rows).
 *
 *  The four buckets are plural messages rather than `${n}<unit> ago` concatenations: the
 *  English abbreviations do not inflect, but Russian and Vietnamese need the count to
 *  choose a form, and only ICU can hand them that choice. `#` also renders the number
 *  through the reader's locale, so a four-digit day count groups correctly. */
export function ago(unixSecs: string | undefined, t: Translate): string {
  const stamped = Number(unixSecs ?? "0");
  if (!stamped) return "—";
  const secs = Math.max(0, Math.floor(Date.now() / 1000) - stamped);
  if (secs < 60) return t("admin.ago.seconds", { n: secs });
  if (secs < 3600) return t("admin.ago.minutes", { n: Math.floor(secs / 60) });
  if (secs < 86_400) return t("admin.ago.hours", { n: Math.floor(secs / 3600) });
  return t("admin.ago.days", { n: Math.floor(secs / 86_400) });
}

/** The role vocabulary, least→most privileged (matches the domain `Role`). */
export const ROLES = ["investor", "operator", "admin", "owner"] as const;

// Wire vocabularies that also reach the screen as labels. The wire value is what goes
// back to the API and never changes; these maps only decide what a reader sees.
//
// Each lookup is guarded by the set rather than interpolated straight into `t()`: a
// missing key resolves to the key itself, so an unrecognised state the hub adds later
// would render as `admin.state.reconciling` in a table cell. Falling back to the raw
// value keeps that failure to the same shape it has today — a bare lowercase word.
const KNOWN_ROLES: ReadonlySet<string> = new Set(ROLES);

const KNOWN_STATUSES: ReadonlySet<string> = new Set([
  "healthy",
  "degraded",
  "error",
  "active",
  "disabled",
  "onboarding",
  "staged",
  "blocked",
]);

const KNOWN_STATES: ReadonlySet<string> = new Set([
  "draft",
  "open",
  "closed",
  "queued",
  "processing",
  "completed",
  "failed",
  "cancelled",
]);

/** A `Role` wire value as a reader sees it. Lowercase, like the wire value — the badges
 *  and table cells that render it carry `capitalize`, so the case stays a CSS decision. */
export function roleLabel(role: string, t: Translate): string {
  return KNOWN_ROLES.has(role) ? t(`admin.role.${role}`) : role;
}

/** A health/lifecycle status as a reader sees it (see {@link roleLabel} on casing). */
export function statusLabel(status: string, t: Translate): string {
  return KNOWN_STATUSES.has(status) ? t(`admin.status.${status}`) : status;
}

/** An allocation / withdrawal / payout state as a reader sees it. */
export function stateLabel(state: string, t: Translate): string {
  return KNOWN_STATES.has(state) ? t(`admin.state.${state}`) : state;
}

// Chain rails, as the console names them. The network codes are proper nouns, but the
// words beside them ("Chain", "Open Network") are prose, so the whole label goes through
// the catalogue rather than being half-translated. Two screens render these — Revenue's
// rail chips and Treasury's per-rail cards — and they must agree.
const RAIL_LABEL_KEYS: Record<string, string> = {
  bep20: "admin.rail.bep20",
  trc20: "admin.rail.trc20",
  ton: "admin.rail.ton",
  polygon: "admin.rail.polygon",
};

/** A rail's display name; a rail the hub adds later falls back to its bare wire code. */
export function railLabel(network: string, t: Translate): string {
  const key = RAIL_LABEL_KEYS[network];
  return key ? t(key) : network;
}

/** Tailwind token classes for a lifecycle/health status pill. */
export function statusTone(status: string): string {
  switch (status) {
    case "active":
    case "healthy":
      return "text-main-accent-t2";
    case "onboarding":
    case "degraded":
    case "staged":
      return "text-main-accent-t3";
    case "blocked":
    case "disabled":
    case "error":
      return "text-destructive";
    default:
      return "text-muted-foreground";
  }
}
