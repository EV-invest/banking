"use client";

// The Holders section's second write: hand part of the company's stake to an investor.
// The supply does not move — units leave the company's row and land on the investors'
// row — so it sits beside "Pin cap to issued" rather than under "Issue units", which
// grows the supply. Two clicks, like the pin: the form REVIEWS, and the confirmation
// shows the exact move ("13,000.00 units: Company → ann@…") before it sends.
//
// The draft, the retry key and the result live here rather than in the form, because the
// confirmation step sits between the two and must see the same draft the form built.

import { ArrowRightLeft, TriangleAlert } from "lucide-react";
import { useRef, useState } from "react";

import { useLocale, useT } from "@evinvest/i18n/react";
import { Button, Spinner } from "@evinvest/uikit";

import { transferCompanyStake } from "@/entities/admin/api/admin-client";
import type { Allocation, UnitHolders } from "@/shared/contracts/admin";
import { errorMessage } from "@/shared/lib/api-client";
import { TAG } from "@/shared/lib/cache-tags";
import { formatUnits, toBaseUnits } from "@/shared/lib/money";
import { revalidateTag } from "@/shared/lib/resource";
import type { SubmissionKey } from "@/views/admin/allocations/lib/issuance";
import { EMPTY_TRANSFER_DRAFT, afterTransferred, transferKeyFor, transferStakeBody, type TransferDraft } from "@/views/admin/allocations/lib/transfer-stake";
import { IssuanceResult, type IssuanceOutcome } from "@/views/admin/allocations/ui/issuance-result";
import { TransferStakeForm } from "@/views/admin/allocations/ui/transfer-stake-form";

type Step = "closed" | "editing" | "confirming";

export function TransferStakeAction({ allocation, holders }: { allocation: Allocation; holders: UnitHolders }) {
  const t = useT();
  const locale = useLocale();
  const [step, setStep] = useState<Step>("closed");
  const [draft, setDraft] = useState<TransferDraft>(EMPTY_TRANSFER_DRAFT);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [last, setLast] = useState<IssuanceOutcome | null>(null);
  // Read and written only inside the submit handler, never during render: the key must
  // survive a failed attempt without triggering one, which is exactly what a ref is for.
  const submission = useRef<SubmissionKey | null>(null);

  // A `queued` mint to the company is not on the ledger yet, so it cannot be handed over
  // yet either — the settled figure is the only one the hub would accept against.
  const nothingHeld = toBaseUnits(holders.company_units) <= 0n;

  const send = async () => {
    const key = transferKeyFor(submission.current, allocation.service, draft);
    submission.current = key;
    const body = transferStakeBody(allocation.service, draft, holders.company_units, key.key);
    if (!body || !draft.recipient) return;
    setBusy(true);
    setError(null);
    try {
      const issuance = await transferCompanyStake(body);
      setLast({ issuance, holderLabel: draft.recipient.label });
      // The split moves (company down, investors up) and the mark's `company_units` with
      // it; the catalog is named because the product cards read the stake from it. Named
      // even for a `queued` row — the resource's own cadence picks the posted leg up.
      revalidateTag(TAG.adminUnitHolders, TAG.nav, TAG.catalog);
      // The recipient stays for the next hand-over in the series; the key is retired so
      // an identical figure typed again is a new transfer, not a de-duplicated retry.
      setDraft(afterTransferred);
      submission.current = null;
      setStep("editing");
    } catch (e) {
      setError(errorMessage(e, t));
      setStep("editing");
    } finally {
      setBusy(false);
    }
  };

  const close = () => {
    setStep("closed");
    setError(null);
  };

  return (
    <div className="space-y-2">
      {step === "closed" && (
        <Button type="button" variant="outline" size="sm" className="w-full" disabled={nothingHeld} onClick={() => setStep("editing")}>
          <ArrowRightLeft className="size-3.5" />
          {t("admin.alloc.transfer.action")}
        </Button>
      )}
      {step === "editing" && <TransferStakeForm draft={draft} companyUnits={holders.company_units} onChange={setDraft} onReview={() => setStep("confirming")} onCancel={close} />}
      {step === "confirming" && draft.recipient && (
        <div className="space-y-2 rounded-lg border border-border bg-secondary p-3">
          <p className="text-xs tabular-nums">{t("admin.alloc.transfer.confirm", { units: formatUnits(draft.units.trim(), locale), holder: draft.recipient.label })}</p>
          <div className="flex gap-2">
            <Button type="button" variant="outline" size="sm" className="flex-1" disabled={busy} onClick={() => setStep("editing")}>
              {t("ui.back")}
            </Button>
            <Button type="button" size="sm" className="flex-1" disabled={busy} onClick={send}>
              {busy ? <Spinner aria-hidden /> : null}
              {t("admin.alloc.transfer.submit")}
            </Button>
          </div>
        </div>
      )}
      {nothingHeld && step === "closed" && <p className="text-xs text-ink-soft">{t("admin.alloc.transfer.nothingHeld")}</p>}
      {last && <IssuanceResult outcome={last} kind="transfer" />}
      {error && (
        <p className="flex items-center gap-2 text-xs text-accent-error">
          <TriangleAlert className="size-3.5" /> {error}
        </p>
      )}
    </div>
  );
}
