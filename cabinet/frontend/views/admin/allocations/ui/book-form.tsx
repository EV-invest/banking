"use client";

// The book's terms as a form: the open switch and the taker fee in front, the tick, lot
// and slippage folded under "advanced" — three figures an operator sets once and should
// not have to read past on every visit. Seeded from the policy as it stands (`key`ed on
// its `updated_at` by the panel, so a save re-seeds it from the answer).
//
// The unbacked-trading acknowledgement is decided here too, before the submit: on a
// product held `in_kind` the hub refuses to open the book without it, so the button stays
// off and the reason is printed beside the tick rather than relayed as a 412.

import { CheckCircle2, ChevronDown, Loader2 } from "lucide-react";
import { useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Button, Collapsible, CollapsibleContent, CollapsibleTrigger, Input, Switch } from "@evinvest/uikit";

import type { Allocation } from "@/shared/contracts/admin";
import type { BookPolicy, SetBookPolicyBody } from "@/shared/contracts/book";
import { cn } from "@/shared/lib/cn";
import { backingOf } from "@/views/admin/allocations/lib/backing";
import { bookPolicyDraft, bookPolicyProblem, hasAdvancedTerms, needsAcknowledgement, setBookPolicyBody, type BookPolicyDraft } from "@/views/admin/allocations/lib/book-policy";
import { BookUnbackedAck } from "@/views/admin/allocations/ui/book-unbacked-ack";

const TEAL_CTA = "bg-primary text-on-primary hover:bg-primary/90";

export function BookForm({ allocation, policy, busy, saved, onSubmit }: { allocation: Allocation; policy: BookPolicy | null; busy: boolean; saved: boolean; onSubmit: (body: SetBookPolicyBody) => Promise<boolean> }) {
  const t = useT();
  const [draft, setDraft] = useState<BookPolicyDraft>(() => bookPolicyDraft(policy));
  // Opened by default when any advanced figure is already set — a value the operator
  // chose should not hide from them.
  const [advanced, setAdvanced] = useState(() => hasAdvancedTerms(policy));
  const problem = bookPolicyProblem(draft);
  const unacknowledged = needsAcknowledgement(draft, allocation);
  const edit = (patch: Partial<BookPolicyDraft>) => setDraft((d) => ({ ...d, ...patch }));

  const submit = async () => {
    const body = setBookPolicyBody(allocation.service, draft);
    if (body && !unacknowledged) await onSubmit(body);
  };

  return (
    <div className="space-y-3 rounded-lg border border-border bg-secondary p-3">
      {/* Not a `<label>`: the switch is a button, and the caption would be a second target. */}
      <div className="flex items-center justify-between gap-3">
        <div>
          <p className="text-sm font-medium">{t("admin.alloc.book.field.open")}</p>
          <p className="text-xs text-ink-soft">{t(draft.open ? "admin.alloc.book.openHint" : "admin.alloc.book.closedHint")}</p>
        </div>
        <Switch checked={draft.open} onCheckedChange={(open) => edit({ open })} disabled={busy} aria-label={t("admin.alloc.book.field.open")} />
      </div>

      <Field label={t("admin.alloc.book.field.takerFee")} value={draft.takerFeePct} onChange={(takerFeePct) => edit({ takerFeePct })} problem={problem === "fee" ? t("admin.alloc.book.problem.fee") : null} hint={t("admin.alloc.book.takerFeeHint")} />

      <BookUnbackedAck checked={draft.allowUnbackedTrading} onChange={(allowUnbackedTrading) => edit({ allowUnbackedTrading })} backing={backingOf(allocation)} required={unacknowledged} disabled={busy} />

      <Collapsible open={advanced} onOpenChange={setAdvanced}>
        <CollapsibleTrigger className="flex w-full items-center justify-between rounded-md py-1 text-xs font-medium text-ink-soft outline-none transition-colors hover:text-ink focus-visible:ring-2 focus-visible:ring-ring">
          {t("admin.alloc.book.advanced")}
          <ChevronDown className={cn("size-3.5 transition-transform", advanced && "rotate-180")} />
        </CollapsibleTrigger>
        <CollapsibleContent className="grid gap-2.5 pt-2">
          <Field label={t("admin.alloc.book.field.tick")} value={draft.tick} onChange={(tick) => edit({ tick })} problem={problem === "tick" ? t("admin.alloc.book.problem.decimal") : null} hint={t("admin.alloc.book.keepHint")} />
          <Field label={t("admin.alloc.book.field.lot")} value={draft.lot} onChange={(lot) => edit({ lot })} problem={problem === "lot" ? t("admin.alloc.book.problem.decimal") : null} hint={t("admin.alloc.book.keepHint")} />
          <Field label={t("admin.alloc.book.field.slippage")} value={draft.slippagePct} onChange={(slippagePct) => edit({ slippagePct })} problem={problem === "slippage" ? t("admin.alloc.book.problem.fee") : null} hint={t("admin.alloc.book.slippageHint")} />
        </CollapsibleContent>
      </Collapsible>

      <Button type="button" className={cn("w-full", TEAL_CTA)} disabled={busy || problem !== null || unacknowledged} onClick={submit}>
        {busy ? <Loader2 className="size-4 animate-spin" /> : null}
        {t("admin.alloc.book.submit")}
      </Button>
      {saved && (
        <p className="flex items-center gap-2 text-xs text-positive">
          <CheckCircle2 className="size-3.5" /> {t("admin.alloc.book.saved")}
        </p>
      )}
    </div>
  );
}

function Field({ label, value, onChange, problem, hint }: { label: string; value: string; onChange: (value: string) => void; problem: string | null; hint: string }) {
  return (
    <label className="flex flex-col gap-1.5">
      <span className="text-xs text-ink-soft">{label}</span>
      <Input inputMode="decimal" value={value} onChange={(e) => onChange(e.target.value)} className="w-full tabular-nums" />
      <span className={cn("text-xs", problem ? "text-accent-error" : "text-ink-soft")}>{problem ?? hint}</span>
    </label>
  );
}
