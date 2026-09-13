"use client";

// Open a payment order: two ends, an amount, a reason — then a second look, then the click.
//
// Nothing here moves money. Submitting OPENS an order: the plane decides who has to agree
// (the owners' consilium for fund-owned money, the investor alone for their own claim),
// emails them, and executes only once they have. Every word on this surface has to carry
// that, because the failure it invites is an operator reading "opened" as "sent".

import { useId, useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Button, Field, FieldDescription, FieldError, FieldLabel, Input, Textarea } from "@evinvest/uikit";

import { openPayment } from "@/entities/payment/model/payment-resource";
import type { Payment } from "@/shared/contracts/payments";
import { errorMessage } from "@/shared/lib/api-client";
import { classifyConsiliumRefusal, coolingOffLiftsAt, type ConsiliumRefusal } from "@/shared/lib/consilium-refusal";
import { ResourceError } from "@/shared/ui/resource-error";
import { EMPTY_END, END_KINDS, PARTY_KINDS, REASON_MAX_BYTES, draftProblem, reasonBytes, toRequest, type EndDraft } from "@/views/admin/payments/lib/terms";
import { EndPicker } from "@/views/admin/payments/ui/end-picker";
import { OpenedReceipt } from "@/views/admin/payments/ui/opened-receipt";
import { ReviewPanel, TermsPreview } from "@/views/admin/payments/ui/terms-preview";
import { RefusalNotice } from "@/views/admin/ui/refusal-notice";

export function OpenPaymentForm() {
  const t = useT();
  const [source, setSource] = useState<EndDraft>(EMPTY_END);
  const [destination, setDestination] = useState<EndDraft>({ ...EMPTY_END, kind: "revenue" });
  const [amount, setAmount] = useState("");
  const [reason, setReason] = useState("");
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  /** A refusal we have specific words for, with the deadline resolved at arrival. */
  const [refusal, setRefusal] = useState<{ detail: ConsiliumRefusal; liftsAt: string | null } | null>(null);
  const [opened, setOpened] = useState<Payment | null>(null);

  const ids = useId();
  const amountId = `${ids}-amount`;
  const reasonId = `${ids}-reason`;
  const reasonHintId = `${ids}-reason-hint`;

  const problem = draftProblem(source, destination, amount, reason);
  const touched = amount.trim().length > 0 || reason.trim().length > 0;
  // A problem is shown beside the field it is about, so the field can point at it
  // (`aria-describedby`) and flag itself invalid; anything about the two ends has no
  // single field and stays as the line under the form.
  const shown = problem && touched ? problem : null;
  const amountProblem = shown === "admin.payments.err.enterAmount" ? shown : null;
  const reasonProblem = shown === "admin.payments.err.enterReason" || shown === "admin.payments.err.reasonTooLong" ? shown : null;
  const endsProblem = shown && !amountProblem && !reasonProblem ? shown : null;
  // Any edit reopens the draft: the review restates the terms, and a review of stale ones
  // would be a confirmation of something the operator is no longer looking at.
  const edit = <T,>(set: (v: T) => void) => (v: T) => {
    set(v);
    setConfirming(false);
  };

  const submit = async () => {
    setBusy(true);
    setError(null);
    setRefusal(null);
    try {
      const payment = await openPayment(toRequest(source, destination, amount, reason));
      setOpened(payment);
      setAmount("");
      setReason("");
      setConfirming(false);
    } catch (cause) {
      const detail = classifyConsiliumRefusal(cause);
      // Resolved once, here: the plane sends a duration, and one re-based on every render
      // drifts away from the deadline it describes.
      if (detail) setRefusal({ detail, liftsAt: detail.kind === "cooling-off" ? coolingOffLiftsAt(detail) : null });
      else setError(cause);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="space-y-4">
      <p className="text-xs text-muted-foreground">{t("admin.payments.formNote")}</p>
      {refusal && <RefusalNotice refusal={refusal.detail} liftsAt={refusal.liftsAt} />}
      {opened && <OpenedReceipt payment={opened} onDismiss={() => setOpened(null)} />}
      {error !== null && <ResourceError message={errorMessage(error, t)} />}

      <div className="grid gap-4 sm:grid-cols-2">
        <EndPicker label={t("admin.payments.source")} value={source} onChange={edit(setSource)} kinds={PARTY_KINDS} />
        <EndPicker label={t("ui.destination")} value={destination} onChange={edit(setDestination)} kinds={END_KINDS} />
      </div>

      <TermsPreview source={source} destination={destination} />

      <div className="grid gap-4 sm:grid-cols-2">
        <Field data-invalid={amountProblem !== null || undefined}>
          <FieldLabel htmlFor={amountId}>{t("admin.payments.amountUsdt")}</FieldLabel>
          <Input
            id={amountId}
            value={amount}
            onChange={(e) => edit(setAmount)(e.target.value)}
            inputMode="decimal"
            placeholder="0.00"
            aria-invalid={amountProblem !== null || undefined}
            aria-describedby={amountProblem ? `${amountId}-error` : undefined}
            className="tabular-nums"
          />
          {amountProblem && <FieldError id={`${amountId}-error`}>{t(amountProblem)}</FieldError>}
        </Field>
        <Field className="sm:col-span-2" data-invalid={reasonProblem !== null || undefined}>
          <FieldLabel htmlFor={reasonId}>{t("admin.payments.reason")}</FieldLabel>
          <Textarea
            id={reasonId}
            value={reason}
            onChange={(e) => edit(setReason)(e.target.value)}
            rows={3}
            placeholder={t("admin.payments.placeholder.reason")}
            aria-invalid={reasonProblem !== null || undefined}
            aria-describedby={reasonProblem ? `${reasonId}-error ${reasonHintId}` : reasonHintId}
          />
          <FieldDescription id={reasonHintId} className="text-xs tabular-nums">
            {t("admin.payments.reasonHint", { used: reasonBytes(reason.trim()), max: REASON_MAX_BYTES })}
          </FieldDescription>
          {reasonProblem && <FieldError id={`${reasonId}-error`}>{t(reasonProblem)}</FieldError>}
        </Field>
      </div>

      {endsProblem && <p className="text-xs text-destructive">{t(endsProblem)}</p>}

      {confirming ? (
        <ReviewPanel source={source} destination={destination} amount={amount} reason={reason} busy={busy} onConfirm={() => void submit()} onBack={() => setConfirming(false)} />
      ) : (
        <Button type="button" size="sm" disabled={problem !== null || busy} onClick={() => setConfirming(true)}>
          {t("admin.payments.review")}
        </Button>
      )}
    </div>
  );
}
