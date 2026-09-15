"use client";

// Mint units in kind to one investor or to the company. The holder toggle decides which
// of the two the body names; the picker only appears for an investor, so "company" can
// never be sent alongside a stale `user_id` that was picked and forgotten.

import { Loader2 } from "lucide-react";
import { useRef, useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Button, Input, ToggleGroup, ToggleGroupItem } from "@evinvest/uikit";

import type { IssueUnitsBody } from "@/entities/admin/api/admin-client";
import { cn } from "@/shared/lib/cn";
import { EMPTY_ISSUE_DRAFT, afterIssued, issueDraftProblem, issueUnitsBody, submissionKeyFor, type IssueDraft, type SubmissionKey } from "@/views/admin/allocations/lib/issuance";
import { UserPicker, type PickedUser } from "@/views/admin/allocations/ui/user-picker";

const TEAL_CTA = "bg-main-accent-t1 text-main-black hover:bg-main-accent-t1/90";

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
  const kind = draft.holder?.kind ?? "user";
  const pickedUser: PickedUser | null = draft.holder?.kind === "user" ? { userId: draft.holder.userId, email: draft.holder.label } : null;

  const setKind = (value: string | string[]) => {
    // Toggling to "investor" leaves the holder unset until someone is picked — an
    // investor is a person, not a mode.
    if (value === "company") setDraft((d) => ({ ...d, holder: { kind: "company" } }));
    else if (value === "user") setDraft((d) => ({ ...d, holder: null }));
  };

  const submit = async () => {
    const key = submissionKeyFor(submission.current, service, draft);
    submission.current = key;
    const body = issueUnitsBody(service, draft, key.key);
    if (!body || !draft.holder) return;
    const label = draft.holder.kind === "company" ? t("admin.alloc.issue.holder.company") : draft.holder.label;
    if (await onSubmit(body, label)) {
      // The holder stays for the next issue in the series; the key is retired so that an
      // identical figure typed again is a new mint, not a de-duplicated retry.
      setDraft(afterIssued);
      submission.current = null;
    }
  };

  return (
    <div className="space-y-3 rounded-lg border border-border bg-main-surface p-3">
      <div className="grid gap-2.5">
        {/* Not a `<label>`: the toggle is a pair of buttons, and wrapping them would make
            the caption a third click target. */}
        <div className="flex flex-col gap-1.5">
          <span className="text-xs text-ink-soft">{t("admin.alloc.issue.field.holder")}</span>
          <ToggleGroup type="single" variant="outline" size="sm" value={kind} onValueChange={setKind} className="w-full">
            <ToggleGroupItem value="user" className="flex-1">
              {t("admin.alloc.issue.holder.investor")}
            </ToggleGroupItem>
            <ToggleGroupItem value="company" className="flex-1">
              {t("admin.alloc.issue.holder.company")}
            </ToggleGroupItem>
          </ToggleGroup>
        </div>
        {kind === "user" && <UserPicker value={pickedUser} onPick={(u) => setDraft((d) => ({ ...d, holder: { kind: "user", userId: u.userId, label: u.email || u.userId } }))} />}
        <label className="flex flex-col gap-1.5">
          <span className="text-xs text-ink-soft">{t("admin.alloc.issue.field.units")}</span>
          <Input inputMode="decimal" value={draft.units} onChange={(e) => setDraft((d) => ({ ...d, units: e.target.value }))} className="w-full tabular-nums" />
          {problem === "units" && draft.units.trim() !== "" && <span className="text-xs text-destructive">{t("admin.alloc.issue.problem.units")}</span>}
        </label>
        <label className="flex flex-col gap-1.5">
          <span className="text-xs text-ink-soft">{t("admin.alloc.issue.field.costBasis")}</span>
          <Input inputMode="decimal" value={draft.costBasis} onChange={(e) => setDraft((d) => ({ ...d, costBasis: e.target.value }))} className="w-full tabular-nums" />
          <span className={cn("text-xs", problem === "costBasis" ? "text-destructive" : "text-ink-soft")}>
            {t(problem === "costBasis" ? "admin.alloc.issue.problem.costBasis" : "admin.alloc.issue.costBasisHint")}
          </span>
        </label>
      </div>
      <Button type="button" className={cn("w-full", TEAL_CTA)} disabled={busy || problem !== null} onClick={submit}>
        {busy ? <Loader2 className="size-4 animate-spin" /> : null}
        {t("admin.alloc.issue.submit")}
      </Button>
      {/* The kit dims a disabled button to half opacity, which on a teal fill over navy
          reads as "slightly quieter" rather than "off" — so the button also says why. */}
      {reason && <p className="text-center text-xs text-ink-soft">{t(reason)}</p>}
    </div>
  );
}
