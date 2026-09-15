"use client";

// The desktop Preferences pane: how the cabinet is displayed to the reader — language,
// base currency, time zone. Stored on the account, so every device follows.

import { useT } from "@evinvest/i18n/react";

import { cn } from "@/shared/lib/cn";
import { CARD } from "@/shared/ui/list-card";
import { CURRENCIES, type Form, LANGUAGES, optionsOf, TIMEZONES } from "@/views/settings/lib/form";
import { Field, FieldSkeleton, SectionHeader, ThemedSelect } from "@/views/settings/ui/fields";

export function PreferencesSection({
  loading,
  form,
  onChange,
  fieldErrors,
}: {
  loading: boolean;
  form: Form | null;
  onChange: (key: keyof Form, value: string) => void;
  fieldErrors: Record<string, string>;
}) {
  const t = useT();
  const ready = !loading && !!form;
  return (
    <section className={cn(CARD, "px-6 py-5.5")}>
      <SectionHeader title={t("settings.nav.preferences")} sub={t("settings.preferencesSub")} />
      <div className="flex flex-wrap gap-x-4.5 gap-y-4">
        <Field label={t("lang.switch")} hint={t("settings.hint.language")}>
          {ready ? <ThemedSelect value={form.language} onChange={(v) => onChange("language", v)} options={LANGUAGES} placeholder={t("settings.selectLanguage")} error={fieldErrors.language} /> : <FieldSkeleton />}
        </Field>
        {/* The currency is stored and nothing in the cabinet renders in it yet, so the hint
            says what it is used for rather than promising a conversion. */}
        <Field label={t("settings.baseCurrency")} hint={t("settings.hint.baseCurrency")}>
          {ready ? <ThemedSelect value={form.base_currency} onChange={(v) => onChange("base_currency", v)} options={optionsOf(CURRENCIES, t)} placeholder={t("settings.selectCurrency")} error={fieldErrors.base_currency} /> : <FieldSkeleton />}
        </Field>
        <Field label={t("settings.timeZone")} hint={t("settings.hint.timeZone")}>
          {ready ? <ThemedSelect value={form.timezone} onChange={(v) => onChange("timezone", v)} options={optionsOf(TIMEZONES, t)} placeholder={t("settings.selectTimeZone")} error={fieldErrors.timezone} /> : <FieldSkeleton />}
        </Field>
      </div>
    </section>
  );
}
