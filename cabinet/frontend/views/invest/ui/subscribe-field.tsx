"use client";

// The subscribe form's two parts around the button: the amount field, with the balance it
// is drawn on, and the line under it — what the amount buys, or why it cannot go through.

import { useLocale, useT } from "@evinvest/i18n/react";

import { Input } from "@evinvest/uikit";

import type { FundNav } from "@/shared/contracts";
import { cn } from "@/shared/lib/cn";
import { TipAnchor } from "@/shared/tips";
import { Link } from "@/shared/ui/cabinet-link";
import { formatUnits, formatUsdt, fromBaseUnits } from "@/views/invest/lib/format";
import type { SubscribeCheck } from "@/views/invest/lib/subscribe-check";

/** `available` is `null` while the wallet is unread: the figure and the Max stay off the
 *  label rather than showing a balance nobody has confirmed. */
export function AmountField({ amount, available, onChange }: { amount: string; available: string | null; onChange: (amount: string) => void }) {
  const t = useT();
  const locale = useLocale();
  return (
    <label className="flex min-w-48 flex-1 flex-col gap-1.5">
      <span className="flex items-center justify-between gap-2 text-sm">
        <span className="flex items-center gap-1.5">
          {t("invest.amountUsdt")}
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
      <Input value={amount} onChange={(e) => onChange(e.target.value)} inputMode="decimal" placeholder="0.00" className="w-full" />
    </label>
  );
}

/** One reason at a time, in the order `checkSubscribe` ranks them — the reader gets the
 *  sentence they can act on, not three at once. */
export function SubscribeHint({ check, nav }: { check: SubscribeCheck; nav: FundNav | null }) {
  const t = useT();
  const locale = useLocale();
  const { preview, headroom, issue } = check;
  const unitArgs = (units: bigint) => ({ n: Number(fromBaseUnits(units)), units: formatUnits(fromBaseUnits(units), locale) });

  if (issue === "insufficient") {
    // The fix for this one is not a smaller number, so the way out sits on the line itself.
    return (
      <p className="flex flex-wrap items-center gap-x-2 text-xs text-accent-error">
        {t("invest.insufficientHint")}
        <Link href="/wallet/deposit" className="rounded-sm font-medium text-primary-ink underline underline-offset-2 outline-none focus-visible:ring-2 focus-visible:ring-ring">
          {t("invest.topUp")}
        </Link>
      </p>
    );
  }

  return (
    <p className={cn("text-xs", issue ? "text-accent-error" : "text-ink-soft")}>
      {issue === "dust"
        ? t("invest.dustHint", { nav: formatUsdt(nav?.nav, locale) })
        : issue === "overCap"
          ? t("invest.overCapHint", unitArgs(headroom ?? 0n))
          : preview !== null
            ? t("invest.buysUnits", { ...unitArgs(preview), nav: formatUsdt(nav?.nav, locale) })
            : t("invest.subscribeIdleHint")}
    </p>
  );
}
