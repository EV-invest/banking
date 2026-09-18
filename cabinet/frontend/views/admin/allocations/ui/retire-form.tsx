"use client";

// The fields of a retirement: whose units, how many, at what basis. The mint form's
// mirror — the same picker, the same cost-basis hint — because the operator is undoing
// the same kind of thing. The button here only REVIEWS; the send is the confirmation step
// in `RetireAction`, because this destroys units for good.

import { useId } from "react";

import { useT } from "@evinvest/i18n/react";
import { Button, Field, FieldDescription, FieldError, FieldLabel, Input } from "@evinvest/uikit";

import { retireDraftProblem, type RetireDraft } from "@/views/admin/allocations/lib/retire";
import { UserPicker, type PickedUser } from "@/views/admin/ui/user-picker";

export function RetireForm({ draft, onChange, onReview, onCancel }: { draft: RetireDraft; onChange: (next: RetireDraft) => void; onReview: () => void; onCancel: () => void }) {
  const t = useT();
  const id = useId();
  const problem = retireDraftProblem(draft);
  // Under the button, only what no field already says: an unpicked holder and untouched
  // units have no message of their own, while a malformed figure is flagged at its field.
  const reason = problem === "holder" ? "admin.alloc.issue.reason.holder" : problem === "units" && draft.units.trim() === "" ? "admin.alloc.issue.reason.units" : null;
  const unitsProblem = draft.units.trim() !== "" && problem === "units";
  const pickedUser: PickedUser | null = draft.holder ? { userId: draft.holder.userId, email: draft.holder.label } : null;
  const basisProblem = problem === "costBasis";

  return (
    <div className="space-y-3 rounded-lg border border-border bg-secondary p-3">
      <div className="grid gap-2.5">
        <Field>
          {/* No `htmlFor`: the picker's trigger sits behind a popover, so it is named by reference. */}
          <FieldLabel id={`${id}-holder`}>{t("admin.alloc.issue.field.holder")}</FieldLabel>
          <UserPicker value={pickedUser} onPick={(u) => onChange({ ...draft, holder: { userId: u.userId, label: u.email || u.userId } })} labelledBy={`${id}-holder`} />
        </Field>
        <Field data-invalid={unitsProblem || undefined}>
          <FieldLabel htmlFor={`${id}-units`}>{t("admin.alloc.issue.field.units")}</FieldLabel>
          <Input id={`${id}-units`} inputMode="decimal" value={draft.units} onChange={(e) => onChange({ ...draft, units: e.target.value })} aria-invalid={unitsProblem || undefined} aria-describedby={`${id}-units-hint`} className="tabular-nums" />
          {/* A person's available units are the hub's to know, and its refusal names them. */}
          {unitsProblem ? <FieldError id={`${id}-units-hint`}>{t("admin.alloc.issue.problem.units")}</FieldError> : <FieldDescription id={`${id}-units-hint`}>{t("admin.alloc.retire.investorHint")}</FieldDescription>}
        </Field>
        <Field data-invalid={basisProblem || undefined}>
          <FieldLabel htmlFor={`${id}-basis`}>{t("admin.alloc.issue.field.costBasis")}</FieldLabel>
          <Input id={`${id}-basis`} inputMode="decimal" value={draft.costBasis} onChange={(e) => onChange({ ...draft, costBasis: e.target.value })} aria-invalid={basisProblem || undefined} aria-describedby={`${id}-basis-hint`} className="tabular-nums" />
          {basisProblem ? <FieldError id={`${id}-basis-hint`}>{t("admin.alloc.issue.problem.costBasis")}</FieldError> : <FieldDescription id={`${id}-basis-hint`}>{t("admin.alloc.retire.costBasisHint")}</FieldDescription>}
        </Field>
      </div>
      <div className="flex gap-2">
        <Button type="button" variant="outline" size="sm" className="flex-1" onClick={onCancel}>
          {t("ui.cancel")}
        </Button>
        <Button type="button" size="sm" className="flex-1" disabled={problem !== null} onClick={onReview}>
          {t("admin.alloc.retire.review")}
        </Button>
      </div>
      {reason && <p className="text-center text-xs text-ink-soft">{t(reason)}</p>}
    </div>
  );
}
