"use client";

// When the change binds, and why — the two fields a change has that a policy did not.
//
// The reason is marked required the moment the draft would need the owners: they read it
// in their approval mail and it is part of what they sign, so an operator should learn
// that here, not from a refusal after the click. It is a single line, because that is what
// the plane accepts (`validate_reason` refuses any control character) and what the mail
// and the history row render — a textarea here used to invite a paragraph the plane then
// bounced with a sentence about control characters.

import { useId } from "react";

import { useLocale, useT } from "@evinvest/i18n/react";
import { Field, FieldDescription, FieldError, FieldLabel, Input } from "@evinvest/uikit";

import { cn } from "@/shared/lib/cn";
import { formatMoment } from "@/shared/lib/datetime";
import { MAX_EFFECTIVE_FROM_HORIZON_SECS, type ChangeRequirement } from "@/shared/lib/fee-terms";
import { effectiveFromLifted, localDateTimeValue, noticeFloor, type TermsDraft } from "@/views/admin/fees/lib/schedule";

export function ScheduleFields({
  draft,
  now,
  requirement,
  effectiveFromError,
  reasonError,
  onChange,
  onReasonTouched,
  disabled,
}: {
  draft: TermsDraft;
  /** The clock the floor, the horizon and the preview are measured from — read once by the
   *  caller, so the picker's bounds do not shift under the operator with every render. */
  now: number;
  /** `null` while the rates do not parse — the question has no answer yet. */
  requirement: ChangeRequirement | null;
  /** What is wrong with the moment — only ever "too far ahead"; an early one is previewed. */
  effectiveFromError: string | null;
  /** What is wrong with the reason, once the operator has been near the field. The
   *  caller decides WHEN it is fair to say so; this only says it under the field. */
  reasonError: string | null;
  onChange: <K extends keyof TermsDraft>(field: K, value: TermsDraft[K]) => void;
  onReasonTouched: () => void;
  disabled: boolean;
}) {
  const t = useT();
  const locale = useLocale();
  const ids = useId();
  const whenId = `${ids}-when`;
  const whenHintId = `${whenId}-hint`;
  const whenErrorId = `${whenId}-error`;
  const reasonId = `${ids}-reason`;
  const reasonHintId = `${reasonId}-hint`;
  const reasonErrorId = `${reasonId}-error`;
  const consilium = requirement === "owner_consilium";
  // The picker's bounds are the plane's: no earlier than now (earlier is lifted, not
  // refused, so this is guidance rather than a gate) and no further than the horizon.
  const min = localDateTimeValue(new Date(now * 1000));
  const max = localDateTimeValue(new Date((now + MAX_EFFECTIVE_FROM_HORIZON_SECS) * 1000));
  // Said as a preview, not an error: the plane will lift the moment, and the operator
  // should read the moment it will actually be before the click. Under the owners' path
  // the floor is counted from their approval, which nobody can date yet.
  const lifted = effectiveFromLifted(draft.effectiveFrom, now);

  return (
    <>
      <Field data-invalid={effectiveFromError !== null || undefined}>
        <FieldLabel htmlFor={whenId}>{t("admin.fees.effectiveFrom")}</FieldLabel>
        <Input
          id={whenId}
          type="datetime-local"
          min={min}
          max={max}
          value={draft.effectiveFrom}
          onChange={(e) => onChange("effectiveFrom", e.target.value)}
          disabled={disabled}
          className="tabular-nums"
          aria-invalid={effectiveFromError !== null || undefined}
          aria-describedby={effectiveFromError !== null ? `${whenErrorId} ${whenHintId}` : whenHintId}
        />
        {effectiveFromError !== null && <FieldError id={whenErrorId}>{effectiveFromError}</FieldError>}
        {lifted && effectiveFromError === null && (
          <p role="status" className="text-xs text-main-accent-t3">
            {consilium ? t("admin.fees.effectiveFromLiftedConsilium") : t("admin.fees.effectiveFromLifted", { floor: formatMoment(String(noticeFloor(now)), locale) })}
          </p>
        )}
        <FieldDescription id={whenHintId}>{t("admin.fees.effectiveFromHint")}</FieldDescription>
      </Field>

      <Field data-invalid={reasonError !== null || undefined}>
        <FieldLabel htmlFor={reasonId}>{consilium ? t("admin.fees.reasonRequiredLabel") : t("admin.fees.reason")}</FieldLabel>
        <Input
          id={reasonId}
          type="text"
          value={draft.reason}
          onChange={(e) => {
            onReasonTouched();
            onChange("reason", e.target.value);
          }}
          onBlur={onReasonTouched}
          disabled={disabled}
          aria-required={consilium || undefined}
          aria-invalid={reasonError !== null || undefined}
          aria-describedby={reasonError !== null ? `${reasonErrorId} ${reasonHintId}` : reasonHintId}
        />
        {reasonError !== null && <FieldError id={reasonErrorId}>{reasonError}</FieldError>}
        <FieldDescription id={reasonHintId}>{t(consilium ? "admin.fees.reasonRequiredHint" : "admin.fees.reasonHint")}</FieldDescription>
      </Field>

      {/* Who will have to agree, said before the click. Withheld while the rates do not
          parse: a requirement computed from a field the form is about to reject would
          name a decision nobody can send. */}
      {requirement && (
        <p className={cn("text-xs", consilium ? "text-main-accent-t3" : "text-muted-foreground")}>
          {t(consilium ? "admin.fees.requirement.consilium" : "admin.fees.requirement.admin")}
        </p>
      )}
    </>
  );
}
