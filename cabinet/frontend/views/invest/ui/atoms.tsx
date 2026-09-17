"use client";

// The small pieces both invest screens are built from: a stat tile, a note, the supply
// bar, and the state badges. They live here rather than in either screen because the
// list and the product page must not describe the same fund two different ways.

import { useLocale, useT } from "@evinvest/i18n/react";
import { Clock, Lock } from "lucide-react";

import { Badge } from "@evinvest/uikit";

import { cn } from "@/shared/lib/cn";
import { TipAnchor } from "@/shared/tips";
import { compactUnits, fractionOfCap } from "@/views/invest/lib/format";

export function Stat({
  label,
  value,
  emphasis,
  tip,
  tone,
  icon,
}: {
  label: string;
  value: string;
  emphasis?: boolean;
  tip?: Parameters<typeof TipAnchor>[0]["anchor"];
  tone?: string;
  icon?: React.ReactNode;
}) {
  return (
    <div className="rounded-lg border border-border bg-secondary p-3">
      <div className="flex items-center gap-1.5">
        <p className="text-xs uppercase tracking-wide text-ink-soft">{label}</p>
        {tip && <TipAnchor anchor={tip} />}
      </div>
      <p className={cn("flex items-center gap-1 tabular-nums", emphasis ? "text-xl font-semibold" : "text-base", tone)}>
        {icon}
        {value}
      </p>
    </div>
  );
}

/** `amber` warns, `muted` explains, `accent` announces — a change on its way that the
 *  reader should know about but nothing is asked of them. */
export function Note({ tone, children }: { tone: "amber" | "muted" | "accent"; children: React.ReactNode }) {
  return (
    <p
      className={cn(
        "rounded-lg border px-3 py-2 text-xs leading-relaxed",
        tone === "amber" && "border-accent-warn/30 bg-accent-warn/5 text-accent-warn",
        tone === "muted" && "border-border bg-ink/5 text-ink-soft",
        tone === "accent" && "border-primary-ink/40 bg-primary-ink/10 text-ink",
      )}
    >
      {children}
    </p>
  );
}

/**
 * How much of a fund's authorised supply is already issued.
 *
 * A fund is not an open tap: an operator sizes it, and once the units are gone it stops
 * minting. Showing that as a bar means a holder can see a fund filling up rather than
 * discovering it at the moment a subscription is refused. Amber past 90% is the only
 * signal — there is nothing to do about it, so it earns attention, not alarm.
 */
export function SupplyBar({ issued, cap, className }: { issued: string | undefined; cap: string | undefined; className?: string }) {
  const t = useT();
  const locale = useLocale();
  const fraction = fractionOfCap(issued, cap);
  const full = fraction >= 1;
  const near = fraction >= 0.9;
  return (
    <div className={cn("space-y-1.5", className)}>
      <div className="flex items-baseline justify-between gap-3 text-xs">
        <span className="text-ink-soft">{t("invest.unitsIssued")}</span>
        <span className={cn("tabular-nums", near ? "font-medium text-accent-warn" : "text-ink-soft")}>
          {compactUnits(issued, locale)} / {compactUnits(cap, locale)}
        </span>
      </div>
      <div className="h-1.5 w-full overflow-hidden rounded-full bg-border">
        {/* Strictly proportional, with no minimum sliver. One unit of a hundred-million
            cap really is nothing, and floor-to-1% would overstate it a millionfold —
            "has this fund started issuing?" is a question for the figure above, which is
            exact, not for a bar whose job is "how full is it?". */}
        <div className={cn("h-full rounded-full", near ? "bg-accent-warn" : "bg-primary-ink")} style={{ width: `${fraction * 100}%` }} />
      </div>
      {full && <p className="text-xs text-accent-warn">{t("invest.fullyIssued")}</p>}
    </div>
  );
}

/** The badges that qualify a product: closed to new money, locked below `invest` by an
 *  operator, or priced off a stale mark. `closed` and `locked` are mutually exclusive by
 *  construction — `isLocked` in `views/invest/lib/product.ts` only ever holds for an open
 *  product — but both may join `stale`. */
export function ProductBadges({ closed, locked, stale }: { closed: boolean; locked: boolean; stale: boolean }) {
  const t = useT();
  if (!closed && !locked && !stale) return null;
  // Inside a `flex-wrap` row here, so these are safe at any length — the same strings are
  // NOT safe on the list card (see `invest-view`), which sets the cap.
  return (
    <div className="flex flex-wrap items-center gap-2">
      {closed && (
        <Badge variant="outline" className="gap-1 border-accent-warn/40 text-accent-warn">
          {t("invest.badge.redeemOnly")}
        </Badge>
      )}
      {locked && (
        <Badge variant="outline" className="gap-1 border-border text-ink-soft">
          <Lock className="size-3" /> {t("invest.badge.locked")}
        </Badge>
      )}
      {stale && (
        <Badge variant="outline" className="gap-1 border-accent-warn/40 text-accent-warn">
          <Clock className="size-3" /> {t("invest.badge.staleNav")}
          <TipAnchor anchor="invest.position.stale-nav" />
        </Badge>
      )}
    </div>
  );
}
