// Display vocabulary for the activity timeline. Amounts are formatted by the cabinet's
// one money module (`@/shared/lib/money`); everything here is the mapping from a wire
// `kind`/`state` onto the badge, tone and wording the Figma `operations` screens use.
//
// The wording is carried as *catalogue keys*, not as English: this is a plain module and
// cannot call `useT()`, so a label resolves at the render site (`kindLabel`, `stateLabel`)
// or takes the translator as an argument. Tone, direction and the settled/failed sets stay
// literal — they are logic, not copy, and no locale changes what a withdrawal is.

import type { Locale, Translate } from "@evinvest/i18n";

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
  /** Catalogue key for the badge glyph — `IN` / `OUT` / `BUY` / `SELL` in the Figma.
   *  `null` for a kind the hub added after this build, whose badge falls back to the
   *  wire id itself (see {@link kindBadge}). */
  badgeKey: string | null;
  /** Catalogue key for the kind's name, or `null` for an unrecognised kind. */
  labelKey: string | null;
  direction: Direction;
  /** Badge tint. Semantic tokens only — these are the accent tiers, not raw colour. */
  tone: string;
}

const KINDS: Record<string, KindMeta> = {
  // `ops.kind.deposit`, not `ui.deposit`: this titles a row in the timeline, where the
  // word is a noun ("a deposit"), while `ui.deposit` is the wallet button, where it is a
  // verb ("deposit funds"). English spells both "Deposit" and hid the difference; German
  // and Russian each have to pick one, and both translators raised it independently.
  deposit: { badgeKey: "ops.badge.in", labelKey: "ops.kind.deposit", direction: "in", tone: "bg-main-accent-t2/15 text-main-accent-t2" },
  withdrawal: { badgeKey: "ops.badge.out", labelKey: "ops.kind.withdrawal", direction: "out", tone: "bg-destructive/15 text-destructive" },
  subscription: { badgeKey: "ops.badge.buy", labelKey: "ops.kind.subscription", direction: "move", tone: "bg-main-accent-t1/15 text-main-accent-t1" },
  redemption: { badgeKey: "ops.badge.sell", labelKey: "ops.kind.redemption", direction: "move", tone: "bg-main-accent-t3/15 text-main-accent-t3" },
  fee: { badgeKey: "ops.badge.fee", labelKey: "ops.kind.fee", direction: "out", tone: "bg-destructive/15 text-destructive" },
};

const UNKNOWN_KIND: KindMeta = { badgeKey: null, labelKey: null, direction: "move", tone: "bg-muted text-muted-foreground" };

/** An unrecognised kind renders neutrally rather than disappearing — a new hub kind is
 *  visible as an unstyled row instead of a silent gap in someone's history. */
export function kindMeta(kind: string | undefined): KindMeta {
  return KINDS[kind ?? ""] ?? UNKNOWN_KIND;
}

/** The badge glyph for a kind. An unrecognised kind wears its own wire id, which is an
 *  identifier rather than copy and so is never translated.
 *
 *  i18n-max: 4 — the badge sits in a `shrink-0` column beside a truncating row title. */
export function kindBadge(kind: string | undefined, t: Translate): string {
  const id = kind ?? "";
  const key = KINDS[id]?.badgeKey;
  return key ? t(key) : id.slice(0, 4).toUpperCase() || "—";
}

/** The kind's name. Same fallback rule as {@link kindBadge}: the wire id when there is
 *  one, and a generic noun only when the hub sent no kind at all. */
export function kindLabel(kind: string | undefined, t: Translate): string {
  const id = kind ?? "";
  const key = KINDS[id]?.labelKey;
  return key ? t(key) : id || t("ops.kind.unknown");
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

// Every lifecycle state the hub sends, as a catalogue key. It used to be the wire
// identifier with its underscores swapped for spaces, leaned on by a `capitalize` class —
// English-shaped twice over: `capitalize` left "Partly Deferred" on a compound state, and
// a translated label is not a lowercase identifier waiting to be title-cased.
const STATES: Record<string, string> = {
  queued: "ops.state.queued",
  processing: "ops.state.processing",
  completed: "ops.state.completed",
  credited: "ops.state.credited",
  charged: "ops.state.charged",
  partly_deferred: "ops.state.partlyDeferred",
  failed: "ops.state.failed",
  cancelled: "ops.state.cancelled",
};

/** A lifecycle state as a human reads it. An unmapped state falls back to the wire
 *  identifier rather than an empty badge, so a new hub state is visible, not invisible.
 *
 *  i18n-max: 12 — badge in a `shrink-0` column; a longer label eats the row title. */
export function stateLabel(state: string | undefined, t: Translate): string {
  const id = state ?? "";
  const key = STATES[id];
  return key ? t(key) : id.replace(/_/g, " ");
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

// The dates used to be pinned to `en-GB`, so a Russian reader's timeline was headed by
// English months and read the clock in an English convention. `en` keeps the British
// ordering the cabinet has always rendered ("12 Mar 2026"); the other four locales get
// their own.
function intlLocale(locale: Locale): string {
  return locale === "en" ? "en-GB" : locale;
}

function daysAgo(unixSeconds: number, now: Date): number {
  return Math.round((startOfDay(now) - startOfDay(new Date(unixSeconds * 1000))) / DAY_MS);
}

function calendarDate(unixSeconds: number, locale: Locale): string {
  return new Date(unixSeconds * 1000).toLocaleDateString(intlLocale(locale), { day: "numeric", month: "short", year: "numeric" });
}

/** The date heading a run of rows sits under: `Today`, `Yesterday`, or `12 Mar 2026`. */
export function dayLabel(unixSeconds: number, t: Translate, locale: Locale, now: Date = new Date()): string {
  if (!unixSeconds) return t("ops.day.undated");
  const days = daysAgo(unixSeconds, now);
  if (days === 0) return t("ops.day.today");
  if (days === 1) return t("ops.day.yesterday");
  return calendarDate(unixSeconds, locale);
}

/** The same day, worded to sit mid-sentence after a rail name: "TON · today 14:32".
 *
 *  Its own keys rather than `dayLabel(...).toLowerCase()`. Lower-casing a *translated*
 *  word is wrong in German, where a noun is capitalised in every position, and it also
 *  mangled the dated case into "12 mar 2026" — a calendar date is not a word, so it is
 *  left exactly as the locale formatted it. */
export function dayLabelInline(unixSeconds: number, t: Translate, locale: Locale, now: Date = new Date()): string {
  if (!unixSeconds) return t("ops.day.undated");
  const days = daysAgo(unixSeconds, now);
  if (days === 0) return t("ops.day.todayInline");
  if (days === 1) return t("ops.day.yesterdayInline");
  return calendarDate(unixSeconds, locale);
}

/** The clock time on a row — the day is already carried by its group heading. */
export function timeLabel(unixSeconds: number, locale: Locale): string {
  if (!unixSeconds) return "—";
  return new Date(unixSeconds * 1000).toLocaleTimeString(intlLocale(locale), { hour: "2-digit", minute: "2-digit" });
}
