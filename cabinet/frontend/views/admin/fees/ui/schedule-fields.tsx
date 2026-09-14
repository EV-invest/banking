"use client";

// When the change binds, and why — the two fields a change has that a policy did not.
//
// The reason is marked required the moment the draft would need the owners: they read it
// in their approval mail and it is part of what they sign, so an operator should learn
// that here, not from a refusal after the click.

import { useId, useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Field, FieldDescription, FieldLabel, Input, Textarea } from "@evinvest/uikit";

import type { ChangeRequirement } from "@/shared/lib/fee-terms";
import type { TermsDraft } from "@/views/admin/fees/lib/schedule";

/** A `datetime-local` value for a moment, in the operator's own zone. */
function localDateTimeValue(at: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${at.getFullYear()}-${pad(at.getMonth() + 1)}-${pad(at.getDate())}T${pad(at.getHours())}:${pad(at.getMinutes())}`;
}

export function ScheduleFields({
  draft,
  requirement,
  onChange,
  disabled,
}: {
  draft: TermsDraft;
  /** `null` while the rates do not parse — the question has no answer yet. */
  requirement: ChangeRequirement | null;
  onChange: <K extends keyof TermsDraft>(field: K, value: TermsDraft[K]) => void;
  disabled: boolean;
}) {
  const t = useT();
  const ids = useId();
  const whenId = `${ids}-when`;
  const reasonId = `${ids}-reason`;
  // Read once: a floor that moved with every render would shift under the picker.
  const [floor] = useState(() => localDateTimeValue(new Date()));
  const consilium = requirement === "owner_consilium";

  return (
    <>
      <Field>
        <FieldLabel htmlFor={whenId}>{t("admin.fees.effectiveFrom")}</FieldLabel>
        <Input id={whenId} type="datetime-local" min={floor} value={draft.effectiveFrom} onChange={(e) => onChange("effectiveFrom", e.target.value)} disabled={disabled} className="tabular-nums" />
        <FieldDescription>{t("admin.fees.effectiveFromHint")}</FieldDescription>
      </Field>

      <Field>
        <FieldLabel htmlFor={reasonId}>{consilium ? t("admin.fees.reasonRequiredLabel") : t("admin.fees.reason")}</FieldLabel>
        <Textarea id={reasonId} value={draft.reason} onChange={(e) => onChange("reason", e.target.value)} rows={3} disabled={disabled} aria-required={consilium || undefined} />
        <FieldDescription>{t(consilium ? "admin.fees.reasonRequiredHint" : "admin.fees.reasonHint")}</FieldDescription>
      </Field>

      {/* Who will have to agree, said before the click. Withheld while the rates do not
          parse: a requirement computed from a field the form is about to reject would
          name a decision nobody can send. */}
      {requirement && (
        <p className={`text-xs ${consilium ? "text-main-accent-t3" : "text-muted-foreground"}`}>
          {t(consilium ? "admin.fees.requirement.consilium" : "admin.fees.requirement.admin")}
        </p>
      )}
    </>
  );
}
