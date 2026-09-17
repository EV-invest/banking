"use client";

// The figures at the top of `/invest/[service]`: a holder's position, or — for a caller
// with no units — the price alone. Beside the way back to the list, which every state of
// the page (found, not found, loading) draws the same.

import { useLocale, useT } from "@evinvest/i18n/react";
import { ArrowLeft, Minus, TrendingDown, TrendingUp, TriangleAlert } from "lucide-react";

import { Card, CardContent, Skeleton } from "@evinvest/uikit";

import type { FundNav, Position } from "@/shared/contracts";
import { TipAnchor } from "@/shared/tips";
import { Link } from "@/shared/ui/cabinet-link";
import { Eyebrow, PageFrame } from "@/shared/ui/page-frame";
import { formatSignedUsdt, formatUnits, formatUsdt, valence, valenceClass } from "@/views/invest/lib/format";
import { Stat } from "@/views/invest/ui/atoms";

export function BackLink() {
  const t = useT();
  return (
    <Link href="/invest" className="inline-flex items-center gap-1.5 rounded-sm text-sm text-ink-soft outline-none transition-colors hover:text-ink focus-visible:ring-2 focus-visible:ring-ring">
      <ArrowLeft className="size-4" />
      {t("invest.allProducts")}
    </Link>
  );
}

export function HoldingStats({ position }: { position: Position }) {
  const t = useT();
  const locale = useLocale();
  // Three tones, not two: a flat P&L is neither a gain nor a loss, and an upward arrow on
  // "0.00" claims a gain that did not happen — the same rule as the dashboard's badge.
  const trend = valence(position.pnl);
  return (
    <div className="grid gap-3 sm:grid-cols-2">
      <Stat label={t("invest.units")} value={formatUnits(position.units, locale)} tip="invest.position.units" />
      <Stat label={t("invest.nav")} value={formatUsdt(position.nav, locale)} tip="invest.position.nav" />
      <Stat label={t("invest.value")} value={`${formatUsdt(position.value, locale)} USDT`} emphasis tip="invest.position.value" />
      <Stat
        label={t("invest.pnl")}
        value={`${formatSignedUsdt(position.pnl, locale)} USDT`}
        tip="invest.position.pnl"
        emphasis
        tone={valenceClass(position.pnl)}
        icon={trend === "loss" ? <TrendingDown className="size-3.5" /> : trend === "gain" ? <TrendingUp className="size-3.5" /> : <Minus className="size-3.5" />}
      />
    </div>
  );
}

/** A product the caller holds nothing in: the price is all there is to show. */
export function PriceOnly({ nav, unmarked }: { nav: FundNav | null; unmarked: boolean }) {
  const t = useT();
  const locale = useLocale();
  return (
    <Card>
      <CardContent className="flex flex-wrap items-center justify-between gap-4 py-6">
        <div className="space-y-1">
          <Eyebrow className="flex items-center gap-1.5">
            {t("invest.navPerUnit")}
            <TipAnchor anchor="invest.position.nav" />
          </Eyebrow>
          <p className="text-2xl font-semibold tabular-nums">{nav ? `${formatUsdt(nav.nav, locale)} USDT` : "—"}</p>
        </div>
        <p className="max-w-sm text-sm text-ink-soft">{t(unmarked ? "invest.notYetValuedHint" : "invest.noUnitsInFund")}</p>
      </CardContent>
    </Card>
  );
}

export function ProductLoading() {
  return (
    <PageFrame>
      <Skeleton className="h-10 w-64" />
      <Skeleton className="h-40 w-full" />
      <Skeleton className="h-32 w-full" />
    </PageFrame>
  );
}

/** No such product for this caller — the hub's answer, or a read that failed before the
 *  catalog could stand in. `error` is the transport's sentence when there is one. */
export function ProductMissing({ service, error }: { service: string; error: string | null }) {
  const t = useT();
  return (
    <PageFrame>
      <BackLink />
      <Card>
        <CardContent className="flex flex-col items-center gap-2 py-16 text-center text-ink-soft">
          <TriangleAlert className="size-6" />
          <p className="text-sm">{error ?? t("invest.notRegistered", { service })}</p>
          <p className="max-w-sm text-xs">{t("invest.notRegisteredHint")}</p>
        </CardContent>
      </Card>
    </PageFrame>
  );
}
