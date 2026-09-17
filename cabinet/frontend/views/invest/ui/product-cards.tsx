"use client";

// The right-hand column of `/invest/[service]`: facts about the fund rather than about the
// holder — its supply and its terms. Kept beside the page rather than inside it so the
// page stays the decision ("what do I do about this product?") and this stays the
// reference material that decision is read against.

import { useLocale, useT } from "@evinvest/i18n/react";

import { Card, CardContent, Spinner } from "@evinvest/uikit";

import type { AccruedFees, FeePolicy, FundNav } from "@/shared/contracts";
import { basisLabel, crystallizationLabel } from "@/shared/lib/fee-terms";
import { pct } from "@/shared/lib/rate";
import { Eyebrow } from "@/shared/ui/page-frame";
import { compactUnits, formatUsdt, isZero } from "@/views/invest/lib/format";
import { companyStakeBps } from "@/views/invest/lib/product";
import { SupplyBar } from "@/views/invest/ui/atoms";
import { FeePendingNote } from "@/views/invest/ui/fee-pending-note";

/**
 * The product's supply, as a fact about the fund rather than about the holder.
 *
 * This is the visible half of the unit cap: a fund is sized by an operator, and once the
 * authorised units are issued it stops minting. Saying so here means "subscription
 * refused" is never the first time a holder hears about it.
 */
export function SupplyCard({ nav }: { nav: FundNav | null }) {
  const t = useT();
  const locale = useLocale();
  if (!nav) {
    return (
      <Card>
        <CardContent className="py-6">
          <Spinner className="text-ink-soft" aria-hidden />
        </CardContent>
      </Card>
    );
  }
  const stake = companyStakeBps(nav);
  return (
    <Card className="h-fit">
      <CardContent className="space-y-4 py-6">
        <p className="text-sm font-semibold">{t("admin.valuation.unitSupply")}</p>
        <SupplyBar issued={nav.units_outstanding} cap={nav.unit_cap} />
        <dl className="space-y-2.5 border-t border-border pt-4 text-sm">
          <Row
            label={t("invest.remainingCapacity")}
            value={t("dash.unitsAmount", { n: Number(nav.remaining_capacity ?? 0), units: compactUnits(nav.remaining_capacity, locale) })}
          />
          {/* The share of the issued supply that is neither this holder's nor the market's —
              stated here, beside the figures it is a share OF, and only when there is one:
              a "0%" row would read as a fact about most funds that is really an absence. */}
          {stake !== null && <Row label={t("invest.companyStake")} value={t("invest.companyStakeValue", { pct: pct(stake), units: compactUnits(nav.company_units, locale) })} />}
          <Row label={t("invest.navPerUnit")} value={`${formatUsdt(nav.nav, locale)} USDT`} />
          <Row label={t("invest.fundAum")} value={nav.aum ? `${formatUsdt(nav.aum, locale)} USDT` : t("invest.notYetValued")} />
        </dl>
        <p className="text-xs text-ink-soft">{t("invest.supplyNote")}</p>
      </CardContent>
    </Card>
  );
}

/**
 * What this fund charges, and what the caller's own holding has run up against it.
 *
 * It sits beside the supply card rather than inside the holding block because the terms
 * apply to a product whether or not the caller is in it — somebody deciding whether to
 * subscribe needs to read the fee before they act, not discover it on the first charge.
 *
 * A product with no policy renders nothing at all. That is deliberate: an absent policy
 * and a policy of zeros are different facts, and a card of zeros reads like a fee that was
 * generously waived rather than a fund that never had one.
 *
 * The accrued figures are shown only to a holder, because they are a statement about a
 * position. `total` is what would be taken if the fee were charged this instant — the
 * number the `Value` stat opposite has NOT been reduced by — so the card says so plainly
 * instead of leaving the two to be reconciled by the reader.
 */
export function FeeCard({ policy, accrued }: { policy: FeePolicy | null; accrued: AccruedFees | null }) {
  const t = useT();
  const locale = useLocale();
  if (!policy?.configured) return null;
  const owed = accrued?.configured ? accrued : null;
  return (
    <Card className="h-fit">
      <CardContent className="space-y-4 py-6">
        <p className="text-sm font-semibold">{t("nav.fees")}</p>
        <dl className="space-y-2.5 text-sm">
          <Row label={t("admin.fees.field.management")} value={t("invest.perAnnum", { pct: pct(policy.management_bps) })} />
          <Row label={t("admin.fees.field.performance")} value={t("invest.ofTheGain", { pct: pct(policy.performance_bps) })} />
          {policy.hurdle_bps ? <Row label={t("admin.fees.field.hurdle")} value={t("invest.hurdleFirst", { pct: pct(policy.hurdle_bps) })} /> : null}
          {/* An unmapped basis/period falls back to the wire identifier — a value the hub
              added that this build has no word for, shown rather than swallowed. */}
          <Row label={t("admin.fees.chargedOn")} value={basisLabel(policy.basis, t)} />
          <Row label={t("invest.lockedIn")} value={crystallizationLabel(policy.crystallization, t)} />
        </dl>

        {/* The terms coming are part of deciding whether to stay in, so a holder and a
            prospect both read them here — before the notice mail, not instead of it. */}
        <FeePendingNote pending={policy.pending} />

        {owed && (
          <div className="space-y-2.5 border-t border-border pt-4">
            {/* Its own heading, so the rows can be labelled `Management` and `Performance`
                without colliding with the identically-named terms above. Long enough labels
                to disambiguate inline would wrap onto two lines in this column. */}
            <Eyebrow>{t("invest.accruedOnHolding")}</Eyebrow>
            <dl className="space-y-2.5 text-sm">
              <Row label={t("admin.fees.field.management")} value={`${formatUsdt(owed.management, locale)} USDT`} />
              <Row label={t("admin.fees.field.performance")} value={`${formatUsdt(owed.performance, locale)} USDT`} />
              {isZero(owed.debt) ? null : <Row label={t("invest.carriedOver")} value={`${formatUsdt(owed.debt, locale)} USDT`} />}
              <Row label={t("ui.total")} value={`${formatUsdt(owed.total, locale)} USDT`} />
              <Row label={t("invest.yourMark")} value={`${formatUsdt(owed.high_water_mark, locale)} USDT`} />
            </dl>
          </div>
        )}

        <p className="text-xs text-ink-soft">{t(owed ? "invest.feeNoteHolder" : "invest.feeNoteProspect")}</p>
      </CardContent>
    </Card>
  );
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-baseline justify-between gap-3">
      <dt className="text-ink-soft">{label}</dt>
      <dd className="font-medium tabular-nums">{value}</dd>
    </div>
  );
}
