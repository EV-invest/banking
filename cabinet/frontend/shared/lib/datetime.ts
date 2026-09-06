// The cabinet's one answer to "when was that" for absolute stamps and deadlines.
//
// It lives in `shared/` rather than beside the governance views because three slices now
// render the same deadline — the two emailed approval pages and the owners' room — and a
// helper each would be three chances for them to disagree about what "expires in 2 days"
// means. Same argument `shared/lib/intl-locale.ts` makes for its one line.
//
// Plain TypeScript: no translator of its own, so anything that produces words takes the
// caller's `t`. Locale-aware formatting goes through `intlLocale`, which is where the
// cabinet decides what `en` means to `Intl`.

import type { Locale, Translate } from "@evinvest/i18n";

import { intlLocale } from "@/shared/lib/intl-locale";

/** An RFC 3339 stamp as an absolute local moment: "12 Mar 2026, 14:03". */
export function formatMoment(iso: string | undefined, locale: Locale): string {
  const at = toDate(iso);
  if (!at) return "—";
  return at.toLocaleString(intlLocale(locale), {
    day: "numeric",
    month: "short",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

/** An RFC 3339 stamp as a date alone: "12 Mar 2026". */
export function formatDay(iso: string | undefined, locale: Locale): string {
  const at = toDate(iso);
  if (!at) return "—";
  return at.toLocaleDateString(intlLocale(locale), { day: "numeric", month: "short", year: "numeric" });
}

/** Whether a deadline has passed. An unparseable or absent stamp is not a passed deadline. */
export function hasExpired(iso: string | undefined): boolean {
  const at = toDate(iso);
  return at !== null && at.getTime() <= Date.now();
}

/**
 * How long is left, in the coarsest unit that is still true.
 *
 * Plural messages rather than `${n} days left`: the English forms do not inflect and the
 * Russian ones do, and only ICU can hand the catalogue that choice. Deliberately coarse —
 * a live countdown on a page about losing your seat would be theatre, and the deadline is
 * 72 hours, not 72 seconds.
 */
export function expiresIn(iso: string | undefined, t: Translate): string {
  const at = toDate(iso);
  if (!at) return "—";
  const ms = at.getTime() - Date.now();
  if (ms <= 0) return t("approval.expiry.passed");
  const minutes = Math.floor(ms / 60_000);
  if (minutes < 60) return t("approval.expiry.minutes", { n: Math.max(1, minutes) });
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return t("approval.expiry.hours", { n: hours });
  return t("approval.expiry.days", { n: Math.floor(hours / 24) });
}

function toDate(iso: string | undefined): Date | null {
  if (!iso) return null;
  const at = new Date(iso);
  return Number.isNaN(at.getTime()) ? null : at;
}
