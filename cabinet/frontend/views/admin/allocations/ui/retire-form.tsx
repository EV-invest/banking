"use client";

// The fields of a retirement: whose units, how many, at what basis. The mint form's
// mirror — the same holder toggle and picker, the same cost-basis hint — because the
// operator is undoing the same kind of thing. The button here only REVIEWS; the send is
// the confirmation step in `RetireAction`, because this destroys units for good.

import { useT } from "@evinvest/i18n/react";
import { Button, Input, ToggleGroup, ToggleGroupItem } from "@evinvest/uikit";

import { cn } from "@/shared/lib/cn";
import { formatUnits } from "@/shared/lib/money";
import { retireDraftProblem, type RetireDraft } from "@/views/admin/allocations/lib/retire";
import { UserPicker, type PickedUser } from "@/views/admin/allocations/ui/user-picker";

export function RetireForm({ draft, companyUnits, onChange, onReview, onCancel }: { draft: RetireDraft; companyUnits: string; onChange: (next: RetireDraft) => void; onReview: () => void; onCancel: () => void }) {
  const t = useT();
  const problem = retireDraftProblem(draft, companyUnits);
  // Under the button, only what no field already says: an unpicked holder and untouched
  // units have no message of their own, while a malformed figure is flagged at its field.
  const reason = problem === "holder" ? "admin.alloc.issue.reason.holder" : problem === "units" && draft.units.trim() === "" ? "admin.alloc.issue.reason.units" : null;
  const unitsProblem = draft.units.trim() !== "" && (problem === "units" || problem === "exceeds") ? problem : null;
  const kind = draft.holder?.kind ?? "user";
  const pickedUser: PickedUser | null = draft.holder?.kind === "user" ? { userId: draft.holder.userId, email: draft.holder.label } : null;

  const setKind = (value: string | string[]) => {
    // Toggling to "investor" leaves the holder unset until someone is picked — an
    // investor is a person, not a mode.
    if (value === "company") onChange({ ...draft, holder: { kind: "company" } });
    else if (value === "user") onChange({ ...draft, holder: null });
  };

  return (
    <div className="space-y-3 rounded-lg border border-border bg-main-surface p-3">
      <div className="grid gap-2.5">
        <div className="flex flex-col gap-1.5">
          <span className="text-xs text-muted-foreground">{t("admin.alloc.issue.field.holder")}</span>
          <ToggleGroup type="single" variant="outline" size="sm" value={kind} onValueChange={setKind} className="w-full">
            <ToggleGroupItem value="user" className="flex-1">
              {t("admin.alloc.issue.holder.investor")}
            </ToggleGroupItem>
            <ToggleGroupItem value="company" className="flex-1">
              {t("admin.alloc.issue.holder.company")}
            </ToggleGroupItem>
          </ToggleGroup>
        </div>
        {kind === "user" && <UserPicker value={pickedUser} onPick={(u) => onChange({ ...draft, holder: { kind: "user", userId: u.userId, label: u.email || u.userId } })} />}
        <label className="flex flex-col gap-1.5">
          <span className="text-xs text-muted-foreground">{t("admin.alloc.issue.field.units")}</span>
          <Input inputMode="decimal" value={draft.units} onChange={(e) => onChange({ ...draft, units: e.target.value })} className="w-full tabular-nums" />
          {/* The company's settled stake is the one figure this form can cap against; an
              investor's available units are the hub's to know, and its refusal names them. */}
          <span className={cn("text-xs tabular-nums", unitsProblem ? "text-destructive" : "text-muted-foreground")}>
            {unitsProblem === "units" ? t("admin.alloc.issue.problem.units") : kind === "company" ? t("admin.alloc.transfer.available", { units: formatUnits(companyUnits) }) : t("admin.alloc.retire.investorHint")}
          </span>
        </label>
        <label className="flex flex-col gap-1.5">
          <span className="text-xs text-muted-foreground">{t("admin.alloc.issue.field.costBasis")}</span>
          <Input inputMode="decimal" value={draft.costBasis} onChange={(e) => onChange({ ...draft, costBasis: e.target.value })} className="w-full tabular-nums" />
          <span className={cn("text-xs", problem === "costBasis" ? "text-destructive" : "text-muted-foreground")}>
            {t(problem === "costBasis" ? "admin.alloc.issue.problem.costBasis" : "admin.alloc.retire.costBasisHint")}
          </span>
        </label>
      </div>
      <div className="flex gap-2">
        <Button type="button" variant="outline" size="sm" className="flex-1" onClick={onCancel}>
          {t("ui.cancel")}
        </Button>
        <Button type="button" size="sm" className="flex-1" disabled={problem !== null} onClick={onReview}>
          {t("admin.alloc.retire.review")}
        </Button>
      </div>
      {reason && <p className="text-center text-xs text-muted-foreground">{t(reason)}</p>}
    </div>
  );
}
