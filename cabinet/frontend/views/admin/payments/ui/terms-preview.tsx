"use client";

// What the plane will decide about this draft, said before the click.
//
// Two facts, both derived and neither editable: which tier the money crosses and who has
// to agree. They are on screen from the first keystroke, not only in the review step,
// because the second one changes WHO reads the reason the operator is typing — every
// owner, or one investor — and that is worth knowing before the sentence is written.

import { useT } from "@evinvest/i18n/react";
import { Button, Spinner } from "@evinvest/uikit";

import { requirementLabel, tierLabel } from "@/entities/payment/lib/format";
import { formatExactUsdt } from "@/shared/lib/money";
import { type EndDraft, previewRequirement, previewTier } from "@/views/admin/payments/lib/terms";
import { draftWords } from "@/views/admin/payments/lib/words";

export function TermsPreview({ source, destination }: { source: EndDraft; destination: EndDraft }) {
  const t = useT();
  const tier = previewTier(destination);
  const requirement = previewRequirement(source);
  return (
    <dl className="grid gap-3 rounded-lg border border-border bg-main-surface p-3 sm:grid-cols-2">
      <div className="min-w-0 space-y-0.5">
        <dt className="text-xs text-muted-foreground">{t("admin.payments.tier")}</dt>
        <dd className="text-sm font-medium text-foreground">{tierLabel(tier, t)}</dd>
        <dd className="text-xs text-muted-foreground">{t(`payment.tier.hint.${tier}`)}</dd>
      </div>
      <div className="min-w-0 space-y-0.5">
        <dt className="text-xs text-muted-foreground">{t("admin.payments.requirement")}</dt>
        <dd className="text-sm font-medium text-foreground">{requirementLabel(requirement, t)}</dd>
        <dd className="text-xs text-muted-foreground">{t(`payment.requirement.hint.${requirement}`)}</dd>
      </div>
    </dl>
  );
}

/**
 * The second, deliberate click. The terms are restated because they are what the
 * approvers will be emailed and what `payload_hash` is computed over — an order is
 * immutable once open, so a typo here means cancelling and starting again, not editing.
 */
export function ReviewPanel({
  source,
  destination,
  amount,
  reason,
  busy,
  onConfirm,
  onBack,
}: {
  source: EndDraft;
  destination: EndDraft;
  amount: string;
  reason: string;
  busy: boolean;
  onConfirm: () => void;
  onBack: () => void;
}) {
  const t = useT();
  const requirement = previewRequirement(source);
  return (
    <div className="space-y-3 rounded-lg border border-main-accent-t3/40 bg-main-accent-t3/5 p-3">
      {/* One sentence, one key: the order of amount, source and destination is a
          per-language decision. `break-words` because an address is 40-plus unbroken
          characters. The amount is the exact wire decimal — the figure the hash covers. */}
      <p className="text-sm tabular-nums break-words">
        {t("admin.payments.reviewSentence", {
          amount: `${formatExactUsdt(amount.trim())} USDT`,
          source: draftWords(source, t),
          destination: draftWords(destination, t),
        })}
      </p>
      {/* The operator's own words, set apart: this is what the approvers will read. */}
      <blockquote className="whitespace-pre-line border-l-2 border-main-accent-t3/60 pl-3 text-sm leading-relaxed text-foreground">{reason.trim()}</blockquote>
      <p className="text-xs text-muted-foreground">{t(`admin.payments.reviewNote.${requirement}`)}</p>
      <div className="flex gap-2">
        <Button type="button" size="sm" disabled={busy} aria-busy={busy} onClick={onConfirm}>
          {busy ? <Spinner aria-hidden /> : null}
          {t("admin.payments.confirmOpen")}
        </Button>
        <Button type="button" size="sm" variant="outline" disabled={busy} onClick={onBack}>
          {t("ui.back")}
        </Button>
      </div>
    </div>
  );
}
