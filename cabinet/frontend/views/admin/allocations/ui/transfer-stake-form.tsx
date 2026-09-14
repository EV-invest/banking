"use client";

// The fields of a stake transfer: who receives, how many units, at what basis. The same
// picker and the same cost-basis hint as the mint form — the operator is doing the same
// kind of thing, to a person rather than to the supply. The button here only REVIEWS; the
// send is the confirmation step in `TransferStakeAction`, because this moves money.

import { useT } from "@evinvest/i18n/react";
import { Button, Input } from "@evinvest/uikit";

import { cn } from "@/shared/lib/cn";
import { formatUnits } from "@/shared/lib/money";
import { transferDraftProblem, type TransferDraft } from "@/views/admin/allocations/lib/transfer-stake";
import { UserPicker } from "@/views/admin/allocations/ui/user-picker";

export function TransferStakeForm({ draft, companyUnits, onChange, onReview, onCancel }: { draft: TransferDraft; companyUnits: string; onChange: (next: TransferDraft) => void; onReview: () => void; onCancel: () => void }) {
  const t = useT();
  const problem = transferDraftProblem(draft, companyUnits);
  // Under the button, only what no field already says: an unpicked recipient and untouched
  // units have no message of their own, while a malformed figure is flagged at its field.
  const reason = problem === "recipient" ? "admin.alloc.transfer.reason.recipient" : problem === "units" && draft.units.trim() === "" ? "admin.alloc.issue.reason.units" : null;
  const unitsProblem = draft.units.trim() !== "" && (problem === "units" || problem === "exceeds") ? problem : null;
  const picked = draft.recipient ? { userId: draft.recipient.userId, email: draft.recipient.label } : null;

  return (
    <div className="space-y-3 rounded-lg border border-border bg-main-surface p-3">
      <div className="grid gap-2.5">
        <div className="flex flex-col gap-1.5">
          <span className="text-xs text-muted-foreground">{t("admin.alloc.transfer.field.recipient")}</span>
          <UserPicker value={picked} onPick={(u) => onChange({ ...draft, recipient: { userId: u.userId, label: u.email || u.userId } })} />
        </div>
        <label className="flex flex-col gap-1.5">
          <span className="text-xs text-muted-foreground">{t("admin.alloc.issue.field.units")}</span>
          <Input inputMode="decimal" value={draft.units} onChange={(e) => onChange({ ...draft, units: e.target.value })} className="w-full tabular-nums" />
          <span className={cn("text-xs tabular-nums", unitsProblem ? "text-destructive" : "text-muted-foreground")}>
            {unitsProblem === "units" ? t("admin.alloc.issue.problem.units") : t("admin.alloc.transfer.available", { units: formatUnits(companyUnits) })}
          </span>
        </label>
        <label className="flex flex-col gap-1.5">
          <span className="text-xs text-muted-foreground">{t("admin.alloc.issue.field.costBasis")}</span>
          <Input inputMode="decimal" value={draft.costBasis} onChange={(e) => onChange({ ...draft, costBasis: e.target.value })} className="w-full tabular-nums" />
          <span className={cn("text-xs", problem === "costBasis" ? "text-destructive" : "text-muted-foreground")}>
            {t(problem === "costBasis" ? "admin.alloc.issue.problem.costBasis" : "admin.alloc.issue.costBasisHint")}
          </span>
        </label>
      </div>
      <div className="flex gap-2">
        <Button type="button" variant="outline" size="sm" className="flex-1" onClick={onCancel}>
          {t("ui.cancel")}
        </Button>
        <Button type="button" size="sm" className="flex-1" disabled={problem !== null} onClick={onReview}>
          {t("admin.alloc.transfer.review")}
        </Button>
      </div>
      {reason && <p className="text-center text-xs text-muted-foreground">{t(reason)}</p>}
    </div>
  );
}
