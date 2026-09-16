// The cabinet's one answer to "when was that" for absolute stamps and deadlines.
//
// It lives in `shared/` rather than beside the governance views because three slices now
// render the same deadline — the two emailed approval pages and the owners' room — and a
// helper each would be three chances for them to disagree about what "expires in 2 days"
// means. Same argument `shared/lib/intl-locale.ts` makes for its one line.
//
// WHAT A STAMP IS HERE: unix SECONDS, as a decimal string, with "0" meaning "no stamp
// yet". That is what the wire actually carries — every timestamp in the contract is an
// `int64` (see `banking/v1/consilium.proto`, "Unix seconds; 0 while pending") and the BFF
// hands it over via `.to_string()`. This module used to parse RFC 3339 instead, which no
// caller was ever given: `new Date("1757000000")` is an Invalid Date, so every date in the
// owners' room, both approval pages and the removal/admission cards rendered "—". Nothing
// caught it because both shapes are `string`.
//
// The parsing itself lives in `shared/lib/unix-stamp.ts` — import-free, and therefore the
// one part of this that `node --test` can actually exercise. This module is the wording
// around it: locales, plurals, dashes.
//
// Plain TypeScript: no translator of its own, so anything that produces words takes the
// caller's `t`. Locale-aware formatting goes through `intlLocale`, which is where the
// cabinet decides what `en` means to `Intl`.

import type { Locale, Translate } from "@evinvest/i18n";

import { intlLocale } from "@/shared/lib/intl-locale";
import { hasUnixStamp, unixStampToDate } from "@/shared/lib/unix-stamp";

const MOMENT: Intl.DateTimeFormatOptions = {
  day: "numeric",
  month: "short",
  year: "numeric",
  hour: "2-digit",
  minute: "2-digit",
};

/** A unix-seconds stamp as an absolute local moment: "12 Mar 2026, 14:03". */
export function formatMoment(stamp: string | null | undefined, locale: Locale): string {
  const at = toDate(stamp);
  if (!at) return "—";
  return at.toLocaleString(intlLocale(locale), MOMENT);
}

/**
 * An RFC 3339 stamp as the local moment {@link formatMoment} renders, with the UTC reading
 * beside it whenever the browser's zone is not UTC: "16 Sept 2026, 16:54 (09:54 UTC)", or
 * "16 Sept 2026, 02:10 (15 Sept, 19:10 UTC)" when the two fall on different days.
 *
 * The one wire that does not carry unix seconds: `/api/admin/deployments` relays what git
 * and the GitHub API say, and both speak RFC 3339. The UTC reading is there because a
 * release time gets compared against CI runs, Flux commits and pod logs, all stamped UTC,
 * while the operators reading it sit in different zones; the local reading stays first
 * because that is how the rest of the cabinet tells time. Kept beside `formatMoment` so
 * the two shapes still share the locale mapping and the fields, and so an unparseable
 * stamp is a dash here too, not "Invalid Date".
 */
export function formatRfc3339Moment(stamp: string | null | undefined, locale: Locale): string {
  if (!stamp) return "—";
  const at = new Date(stamp);
  if (Number.isNaN(at.getTime())) return "—";
  const tag = intlLocale(locale);
  const local = at.toLocaleString(tag, MOMENT);
  // Offset is per instant, so DST is accounted for; zero means the browser already shows UTC.
  if (at.getTimezoneOffset() === 0) return local;
  const sameDay = at.getUTCDate() === at.getDate() && at.getUTCMonth() === at.getMonth() && at.getUTCFullYear() === at.getFullYear();
  const utc = at.toLocaleString(tag, {
    ...(sameDay ? {} : { day: "numeric", month: "short" }),
    hour: "2-digit",
    minute: "2-digit",
    timeZone: "UTC",
    timeZoneName: "short",
  });
  return `${local} (${utc})`;
}

/** A unix-seconds stamp as a date alone: "12 Mar 2026". */
export function formatDay(stamp: string | null | undefined, locale: Locale): string {
  const at = toDate(stamp);
  if (!at) return "—";
  return at.toLocaleDateString(intlLocale(locale), { day: "numeric", month: "short", year: "numeric" });
}

/** Re-exported so a view needing "this stamp, else that one" has a single import. */
export const hasStamp = hasUnixStamp;

/** Whether a deadline has passed. An unparseable or absent stamp is not a passed deadline. */
export function hasExpired(stamp: string | null | undefined): boolean {
  const at = toDate(stamp);
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
export function expiresIn(stamp: string | null | undefined, t: Translate): string {
  const at = toDate(stamp);
  if (!at) return "—";
  const ms = at.getTime() - Date.now();
  if (ms <= 0) return t("approval.expiry.passed");
  const minutes = Math.floor(ms / 60_000);
  if (minutes < 60) return t("approval.expiry.minutes", { n: Math.max(1, minutes) });
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return t("approval.expiry.hours", { n: hours });
  return t("approval.expiry.days", { n: Math.floor(hours / 24) });
}

const toDate = unixStampToDate;
