"use client";

// The subscribe form's one field: the amount, the balance it is drawn on, the action
// beside it, and the line under it — what the amount buys, or why it cannot go through.
//
// One uikit `Field` rather than a `<label>` around the lot: a label wrapping the tip, the
// Max and the input binds to the FIRST labelable element, which is the tip button, and the
// input is left with no accessible name. `htmlFor`/`id` pins the name to the input; the
// balance and the Max are siblings of the label, not part of it.

import { useLocale, useT } from "@evinvest/i18n/react";
import { type ReactNode, useId } from "react";

import { Field, FieldDescription, FieldError, FieldLabel, Input } from "@evinvest/uikit";

import { CustodyNote } from "@/features/kyc";
import type { FundNav } from "@/shared/contracts";
import { TipAnchor } from "@/shared/tips";
import { Link } from "@/shared/ui/cabinet-link";
import { formatUnits, formatUsdt, fromBaseUnits } from "@/views/invest/lib/format";
import type { SubscribeCheck } from "@/views/invest/lib/subscribe-check";

export function SubscribeField({
  amount,
  available,
  check,
  nav,
  action,
  onChange,
}: {
  amount: string;
  /** `null` while the wallet is unread: the figure and the Max stay off the label rather
   *  than showing a balance nobody has confirmed. */
  available: string | null;
  check: SubscribeCheck;
  nav: FundNav | null;
  /** The submit, rendered beside the input so the two stay on one row. */
  action: ReactNode;
  onChange: (amount: string) => void;
}) {
  const t = useT();
  const locale = useLocale();
  const id = useId();
  const hintId = `${id}-hint`;
  const invalid = check.issue !== null;

  return (
    <Field className="gap-2" data-invalid={invalid || undefined}>
      <span className="flex items-center justify-between gap-2">
        <span className="flex items-center gap-1.5 text-sm">
          <FieldLabel htmlFor={id}>{t("invest.amountUsdt")}</FieldLabel>
          <TipAnchor anchor="invest.subscribe.amount" />
        </span>
        {available !== null && (
          <span className="flex items-center gap-2 text-xs text-ink-soft tabular-nums">
            {t("invest.availableUsdt", { amount: formatUsdt(available, locale) })}
            <button type="button" className="rounded-sm font-medium text-primary-ink outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring" onClick={() => onChange(available)}>
              {t("ui.max")}
            </button>
          </span>
        )}
      </span>
      <span className="flex flex-wrap items-center gap-3">
        <Input
          id={id}
          value={amount}
          onChange={(e) => onChange(e.target.value)}
          inputMode="decimal"
          placeholder="0.00"
          aria-invalid={invalid || undefined}
          aria-describedby={hintId}
          className="min-w-48 flex-1 tabular-nums"
        />
        {action}
      </span>
      <SubscribeHint id={hintId} check={check} nav={nav} />
      {/* Next to the money input, not in a footer: that is where a trust line is read. */}
      <CustodyNote />
    </Field>
  );
}

/** One reason at a time, in the order `checkSubscribe` ranks them — the reader gets the
 *  sentence they can act on, not three at once. */
function SubscribeHint({ id, check, nav }: { id: string; check: SubscribeCheck; nav: FundNav | null }) {
  const t = useT();
  const locale = useLocale();
  const { preview, headroom, issue } = check;
  const unitArgs = (units: bigint) => ({ n: Number(fromBaseUnits(units)), units: formatUnits(fromBaseUnits(units), locale) });

  if (issue === "insufficient") {
    // The fix for this one is not a smaller number, so the way out sits on the line itself.
    return (
      <FieldError id={id} className="flex flex-wrap items-center gap-x-2 text-xs">
        {t("invest.insufficientHint")}
        <Link href="/wallet/deposit" className="rounded-sm font-medium text-primary-ink underline underline-offset-2 outline-none focus-visible:ring-2 focus-visible:ring-ring">
          {t("invest.topUp")}
        </Link>
      </FieldError>
    );
  }
  if (issue === "dust") return <FieldError id={id} className="text-xs">{t("invest.dustHint", { nav: formatUsdt(nav?.nav, locale) })}</FieldError>;
  if (issue === "overCap") return <FieldError id={id} className="text-xs">{t("invest.overCapHint", unitArgs(headroom ?? 0n))}</FieldError>;
  return (
    <FieldDescription id={id} className="text-xs tabular-nums">
      {preview !== null ? t("invest.buysUnits", { ...unitArgs(preview), nav: formatUsdt(nav?.nav, locale) }) : t("invest.subscribeIdleHint")}
    </FieldDescription>
  );
}
