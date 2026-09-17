"use client";

import type { ReactNode } from "react";

import { useLocale } from "@evinvest/i18n/react";
import { Card, CardContent, Skeleton } from "@evinvest/uikit";

import { TipAnchor, type TipKey } from "@/shared/tips";
import { NetworkMark } from "@/shared/ui/icons/networks";
import { formatUsd, formatUsdt } from "@/views/admin/lib/format";

/** `unavailable` is the read-failed state: a muted dash, never a formatted `0.00` —
 *  a zero the treasury never reported would be read as a real balance.
 *
 *  `network` is set only on the per-rail cards; the fund-level ones (bank, reserved) name
 *  no chain and get no mark. Every figure here is ledger USDT except the bank line, which
 *  is the mocked USD off-ramp and the one card that keeps the "$". */
export function MoneyCard({
  label,
  network,
  value,
  unit = "USDT",
  hint,
  loading,
  unavailable,
  footer,
  tip,
}: {
  label: string;
  network?: string;
  value: string | undefined;
  unit?: "USDT" | "USD";
  hint?: string;
  loading: boolean;
  unavailable?: boolean;
  footer?: ReactNode;
  tip?: TipKey;
}) {
  const locale = useLocale();
  return (
    <Card>
      <CardContent className="space-y-1 py-5">
        <div className="flex items-center gap-1.5">
          {network && <NetworkMark network={network} className="size-3.5 shrink-0 text-ink-soft" />}
          <p className="text-xs text-ink-soft">{label || "…"}</p>
          {tip && <TipAnchor anchor={tip} />}
        </div>
        {loading ? (
          <Skeleton className="mt-1 h-8 w-28" />
        ) : unavailable ? (
          <p className="text-3xl font-semibold tabular-nums text-ink-soft">—</p>
        ) : (
          <p className="text-3xl font-semibold tabular-nums">
            {unit === "USD" ? (
              formatUsd(value, locale)
            ) : (
              <>
                {formatUsdt(value, locale)} <span className="text-base font-medium text-ink-soft">USDT</span>
              </>
            )}
          </p>
        )}
        {hint && !loading && !unavailable && <p className="text-xs text-positive">{hint}</p>}
        {footer && !loading && footer}
      </CardContent>
    </Card>
  );
}
