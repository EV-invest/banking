// The fund figures as the locale writes them — the cabinet's twin of the site's
// `shared/lib/fund-figures.ts`, so the sign-in panel renders the same shape the hero does:
// "16.4% +" / "16,4 % +" and "$100M". The digits localise; the `$` and `M` do not, on
// purpose — `Intl`'s compact currency would restyle it to "$100m" under en-GB.
//
// Nothing renders a raw value from `FUND_FIGURES`: a surface that wants a figure asks here.

import type { Locale } from "@evinvest/i18n";

// Relative with extensions, like `views/invest/lib/subscribe-check.ts`: the node test
// runner resolves no `@/` alias, and this is one of the modules it runs.
import { FUND_FIGURES } from "../config/fund-figures.ts";
import { intlLocale } from "./intl-locale.ts";

export interface FormattedFundFigures {
  /** "16.4% +" / "16,4 % +" — the locale's percent, then the floor marker. */
  targetIrr: string;
  /** "$100M" — the digits localised, the currency and unit not. */
  closingTarget: string;
  /** The as-of date in prose ("17 Sept 2026"), or absent while the figures are unconfirmed. */
  asOf?: string;
}

export function formatFundFigures(locale: Locale): FormattedFundFigures {
  const { targetIrrPct, closingTargetUsdM, asOf } = FUND_FIGURES;
  const tag = intlLocale(locale);
  const percent = new Intl.NumberFormat(tag, { style: "percent", minimumFractionDigits: 1, maximumFractionDigits: 1 });
  const decimal = new Intl.NumberFormat(tag, { maximumFractionDigits: 0 });
  return {
    targetIrr: `${percent.format(targetIrrPct / 100)} +`,
    closingTarget: `$${decimal.format(closingTargetUsdM)}M`,
    ...(asOf !== undefined && { asOf: formatCalendarDate(asOf, tag) }),
  };
}

/**
 * A calendar date ("2026-09-17") in the locale's form. Parsed at UTC midnight so the day
 * never shifts for a reader west of Greenwich; an unparseable input comes back verbatim
 * rather than as "Invalid Date".
 */
function formatCalendarDate(iso: string, tag: string): string {
  const at = new Date(`${iso}T00:00:00Z`);
  if (Number.isNaN(at.getTime())) return iso;
  return new Intl.DateTimeFormat(tag, { day: "numeric", month: "short", year: "numeric", timeZone: "UTC" }).format(at);
}
