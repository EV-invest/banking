"use client";

// The five terms as inputs: three rates, the basis, the crystallization period.

import type { Translate } from "@evinvest/i18n";
import { useT } from "@evinvest/i18n/react";

import type { RateField, TermsDraft } from "@/views/admin/fees/lib/schedule";
import { Choice, PercentField } from "@/views/admin/fees/ui/fields";
import { Showcase } from "@/views/admin/fees/ui/showcase";

// `value` is the wire enum the money plane stores; `label` is only what a reader sees.
const BASES = [
  { value: "invested_capital", label: (t: Translate) => t("admin.fees.basis.investedCapital", "Invested capital") },
  { value: "market_value", label: (t: Translate) => t("admin.fees.basis.marketValue", "Market value") },
] as const;

const PERIODS = [
  { value: "monthly", label: (t: Translate) => t("admin.fees.period.monthly", "Monthly") },
  { value: "quarterly", label: (t: Translate) => t("admin.fees.period.quarterly", "Quarterly") },
  { value: "semi_annual", label: (t: Translate) => t("admin.fees.period.semiAnnual", "Every 6 months") },
  { value: "annual", label: (t: Translate) => t("admin.fees.period.annual", "Annually") },
] as const;

export function TermsFields({
  draft,
  bps,
  onChange,
  disabled,
}: {
  draft: TermsDraft;
  bps: Record<RateField, number | null>;
  onChange: <K extends keyof TermsDraft>(field: K, value: TermsDraft[K]) => void;
  disabled: boolean;
}) {
  const t = useT();
  return (
    <>
      <div className="grid gap-3 sm:grid-cols-3">
        <PercentField label={t("admin.fees.field.management", "Management")} value={draft.management} onChange={(v) => onChange("management", v)} hint={t("admin.fees.hint.perYear", "per year")} disabled={disabled} />
        <PercentField label={t("admin.fees.field.performance", "Performance")} value={draft.performance} onChange={(v) => onChange("performance", v)} hint={t("admin.fees.hint.ofTheGain", "of the gain")} disabled={disabled} />
        <PercentField label={t("admin.fees.field.hurdle", "Hurdle")} value={draft.hurdle} onChange={(v) => onChange("hurdle", v)} hint={t("admin.fees.hint.zeroForNone", "0 for none")} disabled={disabled} />
      </div>

      <Showcase bps={bps} />

      <Choice label={t("admin.fees.chargedOn", "Charged on")} value={draft.basis} onChange={(v) => onChange("basis", v)} options={BASES} disabled={disabled} />
      <p className="text-xs text-ink-soft">{t("admin.fees.basisNote", "Invested capital is the house default: it does not swell with a mark you posted, so what the manager earns stays independent of the input the manager supplies.")}</p>

      <Choice label={t("admin.fees.lockedIn", "Performance locked in")} value={draft.crystallization} onChange={(v) => onChange("crystallization", v)} options={PERIODS} disabled={disabled} />
      <p className="text-xs text-ink-soft">{t("admin.fees.crystallizationNote", "A price, not a detail — crystallizing more often measurably raises what an investor pays over a fund's life, because each reset locks in gains a later loss can no longer claw back.")}</p>
    </>
  );
}
