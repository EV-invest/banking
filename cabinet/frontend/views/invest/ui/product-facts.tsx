"use client";

// The terms an investor reads before deciding: what the product charges, how money comes
// back out, and when it was last valued. One component for the catalog card and the
// product page's "About" block, so the two never describe the same fund in different words.
//
// The fee line is a headline, not the policy: "2% p.a. + 20% of the gain" and nothing
// about how it is collected. Fees on this platform are issued as units into the fee
// allocation, never deducted from cash (#245), so the line deliberately promises no
// deduction — the full terms, basis and crystallization included, live on `FeeCard`.

import type { Translate } from "@evinvest/i18n";
import { useLocale, useT } from "@evinvest/i18n/react";

import { Skeleton } from "@evinvest/uikit";

import type { FeePolicy, FundNav } from "@/shared/contracts";
import { formatDay } from "@/shared/lib/datetime";
import { pct } from "@/shared/lib/rate";
import { useHydrated } from "@/shared/lib/use-hydrated";
import { type Liquidity } from "@/views/invest/lib/catalog-card";
import { compactUnits } from "@/views/invest/lib/format";

/**
 * The wire's own words for the terms, or the absence of them. A policy that is not
 * `configured` is an absence, not a zero: a "0% + 0%" line would read as a fee waived.
 *
 * `undefined` is "not read yet" and draws a skeleton — distinct from `null`, which is a
 * read that answered with nothing. Collapsing the two printed "Not published yet" on every
 * card's first frame, a false statement about a fee on a money surface.
 */
export function FeeHeadline({ policy }: { policy: FeePolicy | null | undefined }) {
  const t = useT();
  if (policy === undefined) return <Skeleton className="inline-block h-3.5 w-32 align-middle" />;
  if (!policy?.configured) return <>{t("invest.facts.feeUnset", "Not published yet")}</>;
  const words = { management: pct(policy.management_bps), performance: pct(policy.performance_bps), hurdle: pct(policy.hurdle_bps) };
  return <>{(policy.hurdle_bps ? t("invest.facts.feeHeadlineHurdle", "{management} p.a. + {performance} of the gain above {hurdle} p.a.", words) : t("invest.facts.feeHeadline", "{management} p.a. + {performance} of the gain", words))}</>;
}

const liquidityWords = (t: Translate): Record<Liquidity, string> => ({
  navQueued: t("invest.facts.liquidityNav", "Redeem at NAV, queued"),
  navOrBook: t("invest.facts.liquidityNavOrBook", "Redeem at NAV (queued) or trade on the book"),
  book: t("invest.facts.liquidityBook", "Sold on the book"),
});

/**
 * The date behind the NAV. `posted_at` is 0 until an operator marks the fund, and until
 * then the price is the bootstrap 1.0 — a fact worth a word beside the figure, since a
 * "NAV" with no date behind it reads as a price the market set.
 */
export function MarkDate({ nav }: { nav: FundNav | null }) {
  const t = useT();
  const locale = useLocale();
  // A NAV read on the server is in the HTML; until hydration is done the day is UTC's, so
  // the server's render and the hydrating one name the same day.
  const zone = useHydrated() ? undefined : "UTC";
  if (!nav) return <>—</>;
  const stamp = String(nav.posted_at ?? "0");
  return <>{stamp === "0" ? t("invest.notYetValued", "Not yet valued") : t("invest.facts.markedOn", "Marked {date}", { date: formatDay(stamp, locale, zone) })}</>;
}

export function FactRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex items-baseline justify-between gap-3">
      <dt className="shrink-0 text-ink-soft">{label}</dt>
      <dd className="text-right font-medium tabular-nums">{children}</dd>
    </div>
  );
}

/**
 * The rows both surfaces share. `extended` adds the valuation date and the remaining
 * supply as rows — the card carries the date under its NAV figure and draws the supply
 * as `SupplyBar`, so those two are opted into by the page alone.
 */
export function ProductFacts({ policy, nav, liquidity, extended, className }: { policy: FeePolicy | null | undefined; nav: FundNav | null; liquidity: Liquidity | undefined; extended?: boolean; className?: string }) {
  const t = useT();
  const locale = useLocale();
  return (
    <dl className={className}>
      <FactRow label={t("invest.facts.fees", "Fees")}>
        <FeeHeadline policy={policy} />
      </FactRow>
      {/* Unread until the book policy answers: the line flips between "queued" and "or
          trade on the book" otherwise, and a term that changes on screen reads as a lie. */}
      <FactRow label={t("invest.facts.liquidity", "Liquidity")}>{liquidity === undefined ? <Skeleton className="inline-block h-3.5 w-28 align-middle" /> : liquidityWords(t)[liquidity]}</FactRow>
      {extended && (
        <>
          <FactRow label={t("invest.facts.lastValuation", "Last valuation")}>
            <MarkDate nav={nav} />
          </FactRow>
          <FactRow label={t("invest.facts.capacity", "Capacity left")}>
            {nav ? t("dash.unitsAmount", "{n, plural, one {{units} unit} other {{units} units}}", { n: Number(nav.remaining_capacity ?? 0), units: compactUnits(nav.remaining_capacity, locale) }) : "—"}
          </FactRow>
        </>
      )}
    </dl>
  );
}
