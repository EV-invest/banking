"use client";

// The five terms as inputs: three rates, the basis, the crystallization period.

import { useT } from "@evinvest/i18n/react";

import type { RateField, TermsDraft } from "@/views/admin/fees/lib/schedule";
import { Choice, PercentField } from "@/views/admin/fees/ui/fields";
import { Showcase } from "@/views/admin/fees/ui/showcase";

// `value` is the wire enum the money plane stores; `labelKey` is only what a reader sees.
const BASES = [
  { value: "invested_capital", labelKey: "admin.fees.basis.investedCapital" },
  { value: "market_value", labelKey: "admin.fees.basis.marketValue" },
] as const;

const PERIODS = [
  { value: "monthly", labelKey: "admin.fees.period.monthly" },
  { value: "quarterly", labelKey: "admin.fees.period.quarterly" },
  { value: "semi_annual", labelKey: "admin.fees.period.semiAnnual" },
  { value: "annual", labelKey: "admin.fees.period.annual" },
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
        <PercentField label={t("admin.fees.field.management")} value={draft.management} onChange={(v) => onChange("management", v)} hint={t("admin.fees.hint.perYear")} disabled={disabled} />
        <PercentField label={t("admin.fees.field.performance")} value={draft.performance} onChange={(v) => onChange("performance", v)} hint={t("admin.fees.hint.ofTheGain")} disabled={disabled} />
        <PercentField label={t("admin.fees.field.hurdle")} value={draft.hurdle} onChange={(v) => onChange("hurdle", v)} hint={t("admin.fees.hint.zeroForNone")} disabled={disabled} />
      </div>

      <Showcase bps={bps} />

      <Choice label={t("admin.fees.chargedOn")} value={draft.basis} onChange={(v) => onChange("basis", v)} options={BASES} disabled={disabled} />
      <p className="text-xs text-muted-foreground">{t("admin.fees.basisNote")}</p>

      <Choice label={t("admin.fees.lockedIn")} value={draft.crystallization} onChange={(v) => onChange("crystallization", v)} options={PERIODS} disabled={disabled} />
      <p className="text-xs text-muted-foreground">{t("admin.fees.crystallizationNote")}</p>
    </>
  );
}
