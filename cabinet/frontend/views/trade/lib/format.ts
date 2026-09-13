// Display helpers for the terminal. Prices and sizes are decimal strings and go through
// the cabinet's one money module; what is added here is the two shapes a terminal needs
// that no other screen does — a clock for the tape and a signed percent for the ticker.

import type { Locale } from "@evinvest/i18n";

import { intlLocale } from "@/shared/lib/intl-locale";
import { unixStampToDate } from "@/shared/lib/unix-stamp";

export { formatUnits, formatUsdt, fromBaseUnits, isNegative, isZero, toBaseUnits } from "@/shared/lib/money";

/** An int64 as the wire spells it (`number | string`) as the string every formatter takes. */
export function stamp(value: number | string | undefined): string | undefined {
  return value === undefined ? undefined : String(value);
}

/** "14:03:27" — the tape's clock. Seconds matter on a tape; the date does not. */
export function formatClock(at: number | string | undefined, locale: Locale): string {
  const date = unixStampToDate(stamp(at));
  if (!date) return "—";
  return date.toLocaleTimeString(intlLocale(locale), { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

/** "12 Mar, 14:03" — for the order and fill tables, where the day matters too. */
export function formatWhen(at: number | string | undefined, locale: Locale): string {
  const date = unixStampToDate(stamp(at));
  if (!date) return "—";
  return date.toLocaleString(intlLocale(locale), { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
}

/**
 * A signed percent as the wire carries it ("-2.5") → "−2.50%". The Unicode minus is the
 * plus sign's mirror, as in `formatSignedUsd`. An empty string is "no trade to compare
 * against" and renders as a dash rather than as a zero move.
 */
export function formatChange(value: string | undefined): string {
  const raw = (value ?? "").trim();
  if (raw === "") return "—";
  const n = Number(raw);
  if (!Number.isFinite(n)) return raw;
  return `${n < 0 ? "−" : "+"}${Math.abs(n).toFixed(2)}%`;
}

/** Whether a signed percent string is a fall. Exact: reads the sign, not a float. */
export function isFall(value: string | undefined): boolean {
  return (value ?? "").trim().startsWith("-");
}
