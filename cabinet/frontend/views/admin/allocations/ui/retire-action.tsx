"use client";

// The Holders section's destructive write: burn units out of one holder, so the supply
// shrinks. The mirror of "Issue units" above, and it sits with the other stake moves for
// the same reason the transfer does — the operator judges it against the split it will
// change. Two clicks, like the transfer: the form REVIEWS, and the confirmation shows the
// exact burn ("250.00 units from ann@… — the supply shrinks") before it sends.
//
// The draft, the retry key and the result live here rather than in the form, because the
// confirmation step sits between the two and must see the same draft the form built.

import { Flame, TriangleAlert } from "lucide-react";
import { useRef, useState } from "react";

import { useLocale, useT } from "@evinvest/i18n/react";
import { Button, Spinner } from "@evinvest/uikit";

import { retireUnits } from "@/entities/admin/api/admin-client";
import type { Allocation, UnitHolders } from "@/shared/contracts/admin";
import { errorMessage } from "@/shared/lib/api-client";
import { TAG } from "@/shared/lib/cache-tags";
import { formatUnits } from "@/shared/lib/money";
import { revalidateTag } from "@/shared/lib/resource";
import type { SubmissionKey } from "@/views/admin/allocations/lib/issuance";
import { EMPTY_RETIRE_DRAFT, afterRetired, retireAllowed, retireKeyFor, retireUnitsBody, type RetireDraft } from "@/views/admin/allocations/lib/retire";
import { IssuanceResult, type IssuanceOutcome } from "@/views/admin/allocations/ui/issuance-result";
import { RetireForm } from "@/views/admin/allocations/ui/retire-form";
import { RetireGate } from "@/views/admin/allocations/ui/retire-gate";

type Step = "closed" | "editing" | "confirming";

export function RetireAction({ allocation, holders }: { allocation: Allocation; holders: UnitHolders }) {
  const t = useT();
  const locale = useLocale();
  const [step, setStep] = useState<Step>("closed");
  const [draft, setDraft] = useState<RetireDraft>(EMPTY_RETIRE_DRAFT);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [last, setLast] = useState<IssuanceOutcome | null>(null);
  // Read and written only inside the submit handler, never during render: the key must
  // survive a failed attempt without triggering one, which is exactly what a ref is for.
  const submission = useRef<SubmissionKey | null>(null);

  const live = allocation.state !== "closed";
  const holderLabel = draft.holder?.kind === "company" ? t("admin.alloc.issue.holder.company") : (draft.holder?.label ?? "");

  const send = async () => {
    const key = retireKeyFor(submission.current, allocation.service, draft);
    submission.current = key;
    const body = retireUnitsBody(allocation.service, allocation.state, draft, holders.company_units, key.key);
    // Unreachable through the buttons, which disable on the same rules — unless the row
    // went live under an open form. Then the reason is the gate's, not a silent no-op.
    if (!body) {
      setError(t("admin.alloc.retire.closeFirst"));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const issuance = await retireUnits(body);
      setLast({ issuance, holderLabel });
      // The supply shrinks and the mark's `units_outstanding` with it; the catalog is named
      // because the product cards read the split from it. Named even for a `queued` row —
      // the resource's own cadence picks the posted burn up.
      revalidateTag(TAG.adminUnitHolders, TAG.nav, TAG.catalog);
      // The holder stays for the next burn in the series; the key is retired so an
      // identical figure typed again is a new retirement, not a de-duplicated retry. On a
      // live product the override is spent with it, so the gate is shown again rather
      // than a form whose review would lead nowhere.
      setDraft(afterRetired);
      submission.current = null;
      setStep(live ? "closed" : "editing");
    } catch (e) {
      setError(errorMessage(e, t));
      setStep("editing");
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="space-y-2">
      {step === "closed" && (
        <>
          <Button type="button" variant="outline" size="sm" className="w-full" disabled={!retireAllowed(allocation.state, draft.force)} onClick={() => setStep("editing")}>
            <Flame className="size-3.5" />
            {t("admin.alloc.retire.action")}
          </Button>
          {live && <RetireGate force={draft.force} onForce={(force) => setDraft((d) => ({ ...d, force }))} />}
        </>
      )}
      {step === "editing" && <RetireForm draft={draft} companyUnits={holders.company_units} onChange={setDraft} onReview={() => setStep("confirming")} onCancel={() => { setStep("closed"); setError(null); }} />}
      {step === "confirming" && draft.holder && (
        <div className="space-y-2 rounded-lg border border-border bg-secondary p-3">
          <p className="text-xs tabular-nums">{t("admin.alloc.retire.confirm", { units: formatUnits(draft.units.trim(), locale), holder: holderLabel })}</p>
          {live && (
            <p className="flex items-start gap-2 text-xs text-accent-warn">
              <TriangleAlert className="mt-0.5 size-3.5 shrink-0" /> {t("admin.alloc.retire.forceConfirm")}
            </p>
          )}
          <div className="flex gap-2">
            <Button type="button" variant="outline" size="sm" className="flex-1" disabled={busy} onClick={() => setStep("editing")}>
              {t("ui.back")}
            </Button>
            <Button type="button" variant="destructive" size="sm" className="flex-1" disabled={busy} onClick={send}>
              {busy ? <Spinner className="size-3.5" aria-hidden /> : null}
              {t("admin.alloc.retire.submit")}
            </Button>
          </div>
        </div>
      )}
      {last && <IssuanceResult outcome={last} kind="retire" />}
      {error && (
        <p className="flex items-center gap-2 text-xs text-accent-error">
          <TriangleAlert className="size-3.5" /> {error}
        </p>
      )}
    </div>
  );
}
