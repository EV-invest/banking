"use client";

// The whole holding in one band, above the catalog on `/invest`.
//
// Deliberately a single row rather than the two cards it replaced: those were laid out
// side by side with `h-full`, so the shorter one stretched to the taller one's height and
// spent the difference on nothing. Here each block is only as tall as its content and the
// rules between them carry the grouping.

import { useLocale, useT } from "@evinvest/i18n/react";
import { Wallet } from "lucide-react";

import { Button, Card, CardContent } from "@evinvest/uikit";

import { cn } from "@/shared/lib/cn";
import { Link } from "@/shared/ui/cabinet-link";
import { StaggerItem } from "@/shared/ui/motion";
import { formatSignedUsdt, formatUsdt, fromBaseUnits } from "@/views/invest/lib/format";

export function PortfolioBand({ invested, cost, funds, available, queued }: { invested: bigint; cost: bigint; funds: number; available: string | null; queued: number }) {
  const t = useT();
  const locale = useLocale();
  const pnl = invested - cost;
  const loss = pnl < 0n;
  const flat = pnl === 0n;
  // A percentage off a zero cost basis is not "0%", it is undefined — so it is omitted.
  const pct = cost > 0n ? Number((pnl * 10_000n) / cost) / 100 : null;

  return (
    <StaggerItem as={Card}>
      {/* The stacking lives on `max-md:`, not on a `md:` override of a base utility.
          `.flex-col` and `.md\:flex-row` have identical specificity — a media query adds
          none — so whichever Tailwind emits last wins at every width, and it emits the
          base utility last. `md:flex-row` therefore silently lost to `flex-col` and this
          band rendered as a centred column on the desktop it was designed for. Phrased
          this way the desktop state is the unclassed default and nothing competes. */}
      <CardContent className="flex py-5 max-md:flex-col max-md:gap-6 md:items-center">
        <div className="space-y-1.5 md:flex-1">
          <p className="text-xs font-semibold uppercase tracking-widest text-primary-ink">{t("invest.investedValue")}</p>
          <div className="flex flex-wrap items-baseline gap-2.5">
            <span className="text-3xl font-semibold leading-none tabular-nums">{formatUsdt(fromBaseUnits(invested), locale)}</span>
            <span className="text-sm text-ink-soft">USDT</span>
            {!flat && (
              <span className={cn("rounded-full px-2 py-0.5 text-xs font-semibold tabular-nums", loss ? "bg-chart-4/15 text-chart-4" : "bg-positive/15 text-positive")}>
                {formatSignedUsdt(fromBaseUnits(pnl), locale)}
                {pct !== null && ` · ${pct > 0 ? "+" : ""}${pct.toFixed(2)}%`}
              </span>
            )}
          </div>
          <p className="text-xs text-ink-soft">{funds === 0 ? t("invest.noUnitsHeld") : t("invest.acrossFunds", { n: funds })}</p>
        </div>

        <div className="flex flex-wrap gap-8 md:border-l md:border-border md:px-7">
          <BandStat label={t("invest.costBasis")} value={`${formatUsdt(fromBaseUnits(cost), locale)} USDT`} />
          <BandStat label={t("invest.fundsHeld")} value={String(funds)} />
          <BandStat
            label={t("invest.awaitingSettlement")}
            value={queued === 0 ? t("ui.none") : t("admin.valuation.queuedCount", { n: queued })}
            tone={queued > 0 ? "text-accent-warn" : undefined}
          />
        </div>

        <div className="space-y-2 md:border-l md:border-border md:pl-7">
          <p className="flex items-center gap-1.5 text-xs text-ink-soft">
            <Wallet className="size-3.5" /> {t("invest.availableToInvest")}
          </p>
          <p className="text-xl font-semibold tabular-nums">{available === null ? "—" : `${formatUsdt(available, locale)} USDT`}</p>
          <Button asChild type="button" variant="outline" size="sm">
            <Link href="/wallet">{t("invest.topUp")}</Link>
          </Button>
        </div>
      </CardContent>
    </StaggerItem>
  );
}

function BandStat({ label, value, tone }: { label: string; value: string; tone?: string }) {
  return (
    <div className="space-y-1">
      <p className="text-xs text-ink-soft">{label}</p>
      <p className={cn("text-sm font-semibold tabular-nums", tone)}>{value}</p>
    </div>
  );
}
