"use client";

// The desktop General pane (Figma `cabinet/settings`): the editable core user record, one
// labelled field per preference.

import { useT } from "@evinvest/i18n/react";

import { Input } from "@evinvest/uikit";

import { cn } from "@/shared/lib/cn";
import { CARD } from "@/shared/ui/list-card";
import { formatEmail } from "@/views/settings/lib/contact";
import { CURRENCIES, type Form, LANGUAGES, TIMEZONES } from "@/views/settings/lib/form";
import { Field, FieldSkeleton, PhoneField, SectionHeader, ThemedSelect, VerifiedTag } from "@/views/settings/ui/fields";

export function GeneralSection({
  loading,
  form,
  email,
  verified,
  onChange,
  fieldErrors,
}: {
  loading: boolean;
  form: Form | null;
  email: string | null;
  verified: boolean;
  onChange: (key: keyof Form, value: string) => void;
  fieldErrors: Record<string, string>;
}) {
  const t = useT();
  const ready = !loading && !!form;
  return (
    <section className={cn(CARD, "px-6 py-5.5")}>
      <SectionHeader title={t("ui.account")} sub="Your contact details and preferences" />
      <div className="flex flex-wrap gap-x-4.5 gap-y-4">
        <Field label="Legal name">
          {ready ? (
            <div className="min-w-0 flex-1">
              <Input value={form.legal_name} onChange={(e) => onChange("legal_name", e.target.value)} className={fieldErrors.legal_name ? "border-destructive bg-destructive/5" : "border-border bg-main-surface"} />
              {fieldErrors.legal_name && <p className="mt-1 text-xs text-destructive">{fieldErrors.legal_name}</p>}
            </div>
          ) : <FieldSkeleton />}
        </Field>
        <Field label="Preferred name">
          {ready ? (
            <div className="min-w-0 flex-1">
              <Input value={form.preferred_name} onChange={(e) => onChange("preferred_name", e.target.value)} className={fieldErrors.preferred_name ? "border-destructive bg-destructive/5" : "border-border bg-main-surface"} />
              {fieldErrors.preferred_name && <p className="mt-1 text-xs text-destructive">{fieldErrors.preferred_name}</p>}
            </div>
          ) : <FieldSkeleton />}
        </Field>
        <Field label="Email address" trailing={verified ? <VerifiedTag /> : undefined}>
          {loading ? <FieldSkeleton /> : <Input value={formatEmail(email)} readOnly className="border-border bg-main-surface text-muted-foreground" />}
        </Field>
        <Field label="Phone number">
          {ready ? <PhoneField initial={form.phone} onChange={(v) => onChange("phone", v)} error={fieldErrors.phone} /> : <FieldSkeleton />}
        </Field>
        <Field label="Language">
          {ready ? <ThemedSelect value={form.language} onChange={(v) => onChange("language", v)} options={LANGUAGES} placeholder="Select language" error={fieldErrors.language} /> : <FieldSkeleton />}
        </Field>
        <Field label="Base currency">
          {ready ? <ThemedSelect value={form.base_currency} onChange={(v) => onChange("base_currency", v)} options={CURRENCIES} placeholder="Select currency" error={fieldErrors.base_currency} /> : <FieldSkeleton />}
        </Field>
        <Field label="Time zone">
          {ready ? <ThemedSelect value={form.timezone} onChange={(v) => onChange("timezone", v)} options={TIMEZONES} placeholder="Select time zone" error={fieldErrors.timezone} /> : <FieldSkeleton />}
        </Field>
      </div>
    </section>
  );
}
