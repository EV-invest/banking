"use client";

// Mint units in kind to one person. There is no holder toggle any more (#245): the
// company is not a holder, and the reserved `fee` / `fund` allocations are seated by the
// owners' consilium, so the picker is the whole of "who".

import { useRef, useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Button, Input, Spinner } from "@evinvest/uikit";

import type { IssueUnitsBody } from "@/entities/admin/api/admin-client";
import { cn } from "@/shared/lib/cn";
import { EMPTY_ISSUE_DRAFT, afterIssued, issueDraftProblem, issueUnitsBody, submissionKeyFor, type IssueDraft, type SubmissionKey } from "@/views/admin/allocations/lib/issuance";
import { UserPicker, type PickedUser } from "@/views/admin/allocations/ui/user-picker";

const TEAL_CTA = "bg-primary text-on-primary hover:bg-primary/90";

export function IssueForm({ service, busy, onSubmit }: { service: string; busy: boolean; onSubmit: (body: IssueUnitsBody, holderLabel: string) => Promise<boolean> }) {
  const t = useT();
  const [draft, setDraft] = useState<IssueDraft>(EMPTY_ISSUE_DRAFT);
  // Read and written only inside the submit handler, never during render: the key must
  // survive a failed attempt without triggering one, which is exactly what a ref is for.
  const submission = useRef<SubmissionKey | null>(null);

  const problem = issueDraftProblem(draft);
  // Under the button, only what no field already says: an unpicked holder and untouched
  // units have no message of their own, while a malformed figure is flagged at its field.
  const reason = problem === "holder" ? "admin.alloc.issue.reason.holder" : problem === "units" && draft.units.trim() === "" ? "admin.alloc.issue.reason.units" : null;
  const pickedUser: PickedUser | null = draft.holder ? { userId: draft.holder.userId, email: draft.holder.label } : null;

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
        <div className="flex flex-col gap-1.5">
          <span className="text-xs text-ink-soft">{t("admin.alloc.issue.field.holder")}</span>
          <UserPicker value={pickedUser} onPick={(u) => setDraft((d) => ({ ...d, holder: { userId: u.userId, label: u.email || u.userId } }))} />
        </div>
        <label className="flex flex-col gap-1.5">
          <span className="text-xs text-ink-soft">{t("admin.alloc.issue.field.units")}</span>
          <Input inputMode="decimal" value={draft.units} onChange={(e) => setDraft((d) => ({ ...d, units: e.target.value }))} className="w-full tabular-nums" />
          {problem === "units" && draft.units.trim() !== "" && <span className="text-xs text-accent-error">{t("admin.alloc.issue.problem.units")}</span>}
        </label>
        <label className="flex flex-col gap-1.5">
          <span className="text-xs text-ink-soft">{t("admin.alloc.issue.field.costBasis")}</span>
          <Input inputMode="decimal" value={draft.costBasis} onChange={(e) => setDraft((d) => ({ ...d, costBasis: e.target.value }))} className="w-full tabular-nums" />
          <span className={cn("text-xs", problem === "costBasis" ? "text-accent-error" : "text-ink-soft")}>
            {t(problem === "costBasis" ? "admin.alloc.issue.problem.costBasis" : "admin.alloc.issue.costBasisHint")}
          </span>
        </label>
      </div>
      <Button type="button" className={cn("w-full", TEAL_CTA)} disabled={busy || problem !== null} onClick={submit}>
        {busy ? <Spinner aria-hidden /> : null}
        {t("admin.alloc.issue.submit")}
      </Button>
      {/* The kit dims a disabled button to half opacity, which on a teal fill over navy
          reads as "slightly quieter" rather than "off" — so the button also says why. */}
      {reason && <p className="text-center text-xs text-ink-soft">{t(reason)}</p>}
    </div>
  );
}
