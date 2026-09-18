"use client";

// Mint units in kind to one person. There is no holder toggle any more (#245): the
// company is not a holder, and the reserved `fee` / `fund` allocations are seated by the
// owners' consilium, so the picker is the whole of "who".

import { useId, useRef, useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Button, Field, FieldDescription, FieldError, FieldLabel, Input, Spinner } from "@evinvest/uikit";

import type { IssueUnitsBody } from "@/entities/admin/api/admin-client";
import { EMPTY_ISSUE_DRAFT, afterIssued, issueDraftProblem, issueUnitsBody, submissionKeyFor, type IssueDraft, type SubmissionKey } from "@/views/admin/allocations/lib/issuance";
import { UserPicker, type PickedUser } from "@/views/admin/ui/user-picker";

export function IssueForm({ service, busy, onSubmit }: { service: string; busy: boolean; onSubmit: (body: IssueUnitsBody, holderLabel: string) => Promise<boolean> }) {
  const t = useT();
  const id = useId();
  const [draft, setDraft] = useState<IssueDraft>(EMPTY_ISSUE_DRAFT);
  // Read and written only inside the submit handler, never during render: the key must
  // survive a failed attempt without triggering one, which is exactly what a ref is for.
  const submission = useRef<SubmissionKey | null>(null);

  const problem = issueDraftProblem(draft);
  // Under the button, only what no field already says: an unpicked holder and untouched
  // units have no message of their own, while a malformed figure is flagged at its field.
  const reason = problem === "holder" ? "admin.alloc.issue.reason.holder" : problem === "units" && draft.units.trim() === "" ? "admin.alloc.issue.reason.units" : null;
  const pickedUser: PickedUser | null = draft.holder ? { userId: draft.holder.userId, email: draft.holder.label } : null;
  const unitsProblem = problem === "units" && draft.units.trim() !== "";
  const basisProblem = problem === "costBasis";

  const submit = async () => {
    const key = submissionKeyFor(submission.current, service, draft);
    submission.current = key;
    const body = issueUnitsBody(service, draft, key.key);
    if (!body || !draft.holder) return;
    if (await onSubmit(body, draft.holder.label)) {
      // The holder stays for the next issue in the series; the key is retired so that an
      // identical figure typed again is a new mint, not a de-duplicated retry.
      setDraft(afterIssued);
      submission.current = null;
    }
  };

  return (
    <div className="space-y-3 rounded-lg border border-border bg-secondary p-3">
      <div className="grid gap-2.5">
        <Field>
          {/* No `htmlFor`: the picker's trigger sits behind a popover, so it is named by reference. */}
          <FieldLabel id={`${id}-holder`}>{t("admin.alloc.issue.field.holder")}</FieldLabel>
          <UserPicker value={pickedUser} onPick={(u) => setDraft((d) => ({ ...d, holder: { userId: u.userId, label: u.email || u.userId } }))} labelledBy={`${id}-holder`} />
        </Field>
        <Field data-invalid={unitsProblem || undefined}>
          <FieldLabel htmlFor={`${id}-units`}>{t("admin.alloc.issue.field.units")}</FieldLabel>
          <Input id={`${id}-units`} inputMode="decimal" value={draft.units} onChange={(e) => setDraft((d) => ({ ...d, units: e.target.value }))} aria-invalid={unitsProblem || undefined} aria-describedby={unitsProblem ? `${id}-units-error` : undefined} className="tabular-nums" />
          {unitsProblem && <FieldError id={`${id}-units-error`}>{t("admin.alloc.issue.problem.units")}</FieldError>}
        </Field>
        <Field data-invalid={basisProblem || undefined}>
          <FieldLabel htmlFor={`${id}-basis`}>{t("admin.alloc.issue.field.costBasis")}</FieldLabel>
          <Input id={`${id}-basis`} inputMode="decimal" value={draft.costBasis} onChange={(e) => setDraft((d) => ({ ...d, costBasis: e.target.value }))} aria-invalid={basisProblem || undefined} aria-describedby={`${id}-basis-hint`} className="tabular-nums" />
          {basisProblem ? <FieldError id={`${id}-basis-hint`}>{t("admin.alloc.issue.problem.costBasis")}</FieldError> : <FieldDescription id={`${id}-basis-hint`}>{t("admin.alloc.issue.costBasisHint")}</FieldDescription>}
        </Field>
      </div>
      <Button type="button" className="w-full" disabled={busy || problem !== null} onClick={submit}>
        {busy ? <Spinner aria-hidden /> : null}
        {t("admin.alloc.issue.submit")}
      </Button>
      {/* The kit dims a disabled button to half opacity, which on a teal fill over navy
          reads as "slightly quieter" rather than "off" — so the button also says why. */}
      {reason && <p className="text-center text-xs text-ink-soft">{t(reason)}</p>}
    </div>
  );
}
