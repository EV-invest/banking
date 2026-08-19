// Display vocabulary for the activity timeline. Amounts are formatted by the cabinet's
// one money module (`@/shared/lib/money`); everything here is the mapping from a wire
// `kind`/`state` onto the badge, tone and wording the Figma `operations` screens use.

import type { Operation } from "@/shared/contracts";

export { formatUnits, formatUsdt, shortAddress } from "@/shared/lib/money";
// Rails already have one display vocabulary on the wallet surface — a timeline row must
// name a network exactly as the wallet does, so it is re-exported, never re-declared.
export { networkLabel } from "@/views/wallet/lib/format";

export type OperationKind = "deposit" | "withdrawal" | "subscription" | "redemption";

/** The tab set, in the order the filter row presents them. */
export const KIND_FILTERS = ["deposit", "withdrawal", "subscription", "redemption", "fee"] as const;

// Sign policy. Only the movements that actually change what the user holds carry a
// sign: money arriving (deposit), money leaving (withdrawal), and the fund's fee, which
// takes units from the holder and does not give them back. Subscribing and redeeming move
// value *between* the wallet and a fund without either entering or leaving the account, so
// they render unsigned in the neutral tone — the Figma's `MOVE` row. Signing them would
// tell an investor that subscribing lost them money; NOT signing a fee would tell them a
// charge was free.
export type Direction = "in" | "out" | "move";

export interface KindMeta {
  /** The badge glyph text — `IN` / `OUT` / `BUY` / `SELL` in the Figma. */
  badge: string;
  label: string;
  direction: Direction;
  /** Badge tint. Semantic tokens only — these are the accent tiers, not raw colour. */
  tone: string;
}

const KINDS: Record<string, KindMeta> = {
  deposit: { badge: "IN", label: "Deposit", direction: "in", tone: "bg-main-accent-t2/15 text-main-accent-t2" },
  withdrawal: { badge: "OUT", label: "Withdrawal", direction: "out", tone: "bg-destructive/15 text-destructive" },
  subscription: { badge: "BUY", label: "Subscription", direction: "move", tone: "bg-main-accent-t1/15 text-main-accent-t1" },
  redemption: { badge: "SELL", label: "Redemption", direction: "move", tone: "bg-main-accent-t3/15 text-main-accent-t3" },
  fee: { badge: "FEE", label: "Fee", direction: "out", tone: "bg-destructive/15 text-destructive" },
};

/** An unrecognised kind renders neutrally rather than disappearing — a new hub kind is
 *  visible as an unstyled row instead of a silent gap in someone's history. */
export function kindMeta(kind: string | undefined): KindMeta {
  const id = kind ?? "";
  return KINDS[id] ?? { badge: id.slice(0, 4).toUpperCase() || "—", label: id || "Operation", direction: "move", tone: "bg-muted text-muted-foreground" };
}

/** The amount colour that goes with a direction. Neutral moves keep the body colour. */
export function amountTone(direction: Direction): string {
  if (direction === "in") return "text-main-accent-t2";
  if (direction === "out") return "text-destructive";
  return "text-foreground";
}

// `partly_deferred` is settled too: the charge itself completed, and what it could not
// collect became debt on the position rather than an operation still in flight.
const SETTLED = new Set(["credited", "completed", "charged", "partly_deferred"]);
const FAILED = new Set(["failed", "cancelled"]);

/** Still moving — the rows the "In progress" section lifts to the top. */
export function isPending(operation: Operation): boolean {
  const state = operation.state ?? "";
  return !SETTLED.has(state) && !FAILED.has(state);
}

/** A lifecycle state as a human reads it. Every state the hub sends is a lowercase
 *  identifier that `capitalize` alone renders correctly — except the compound ones, where
 *  it would leave the underscore in ("Partly_deferred"). */
export function stateLabel(state: string | undefined): string {
  return (state ?? "").replace(/_/g, " ");
}

/** Badge tint for a lifecycle state, matching the wallet activity screen's vocabulary. */
export function stateTone(state: string | undefined): string {
  switch (state) {
    case "queued":
      return "bg-main-accent-t3/15 text-main-accent-t3";
    case "processing":
      return "bg-main-accent-t1/15 text-main-accent-t1";
    case "completed":
    case "credited":
    case "charged":
      return "bg-main-accent-t2/15 text-main-accent-t2";
    // Part of the charge could not be collected and is carried to the next one — worth
    // the attention tint, since it is the only state where the row's figure is less than
    // what was actually assessed.
    case "partly_deferred":
      return "bg-main-accent-t3/15 text-main-accent-t3";
    case "failed":
      return "bg-destructive/15 text-destructive";
    default:
      return "bg-muted text-muted-foreground";
  }
}

/** Unix seconds on the wire arrive as a string (the BFF renders i64 as text so no
 *  client has to survive 2^53); `0`/absent means the hub never stamped one. */
export function seconds(value: string | number | undefined): number {
  const n = Number(value ?? 0);
  return Number.isFinite(n) ? n : 0;
}

// Grouping and time labels are computed against the viewer's own clock. The view fetches
// in an effect, so the first paint is the skeleton and there is no server/client
// timezone mismatch to hydrate around.
const DAY_MS = 86_400_000;

function startOfDay(date: Date): number {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
}

/** The date heading a run of rows sits under: `Today`, `Yesterday`, or `12 Mar 2026`. */
export function dayLabel(unixSeconds: number, now: Date = new Date()): string {
  if (!unixSeconds) return "Undated";
  const date = new Date(unixSeconds * 1000);
  const days = Math.round((startOfDay(now) - startOfDay(date)) / DAY_MS);
  if (days === 0) return "Today";
  if (days === 1) return "Yesterday";
  return date.toLocaleDateString("en-GB", { day: "numeric", month: "short", year: "numeric" });
}

/** The clock time on a row — the day is already carried by its group heading. */
export function timeLabel(unixSeconds: number): string {
  if (!unixSeconds) return "—";
  return new Date(unixSeconds * 1000).toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" });
}
