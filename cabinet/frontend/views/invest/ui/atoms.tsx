"use client";

// The small pieces both invest screens are built from: a stat tile, a note, the supply
// bar, and the state badges. They live here rather than in either screen because the
// list and the product page must not describe the same fund two different ways.

import { Clock } from "lucide-react";

import { Badge } from "@evinvest/uikit";

import { cn } from "@/shared/lib/cn";
import { TipAnchor } from "@/shared/tips";
import { compactUnits, fractionOfCap } from "@/views/invest/lib/format";

export const TEAL_CTA = "bg-main-accent-t1 text-main-black hover:bg-main-accent-t1/90";

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
    <div className="rounded-lg border border-border bg-main-surface p-3">
      <div className="flex items-center gap-1.5">
        <p className="text-xs uppercase tracking-wide text-muted-foreground">{label}</p>
        {tip && <TipAnchor anchor={tip} />}
      </div>
      <p className={cn("flex items-center gap-1 tabular-nums", emphasis ? "text-xl font-semibold" : "text-base", tone)}>
        {icon}
        {value}
      </p>
    </div>
  );
}

export function Note({ tone, children }: { tone: "amber" | "muted"; children: React.ReactNode }) {
  return (
    <p className={cn("rounded-lg border px-3 py-2 text-xs", tone === "amber" ? "border-main-accent-t3/30 bg-main-accent-t3/5 text-main-accent-t3" : "border-border bg-foreground/5 text-muted-foreground")}>
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
  const fraction = fractionOfCap(issued, cap);
  const full = fraction >= 1;
  const near = fraction >= 0.9;
  return (
    <div className={cn("space-y-1.5", className)}>
      <div className="flex items-baseline justify-between gap-3 text-xs">
        <span className="text-muted-foreground">Units issued</span>
        <span className={cn("tabular-nums", near ? "font-medium text-main-accent-t3" : "text-muted-foreground")}>
          {compactUnits(issued)} / {compactUnits(cap)}
        </span>
      </div>
      <div className="h-1.5 w-full overflow-hidden rounded-full bg-border">
        {/* A non-zero supply always paints at least a sliver — a bar that reads as empty
            when units exist is a lie about a figure the holder can check elsewhere. */}
        <div className={cn("h-full rounded-full", near ? "bg-main-accent-t3" : "bg-main-accent-t1")} style={{ width: `${Math.max(fraction * 100, fraction > 0 ? 1 : 0)}%` }} />
      </div>
      {full && <p className="text-xs text-main-accent-t3">Fully issued — not minting new units.</p>}
    </div>
  );
}

/** The badges that qualify a product: closed to new money, or priced off a stale mark. */
export function ProductBadges({ closed, stale }: { closed: boolean; stale: boolean }) {
  if (!closed && !stale) return null;
  return (
    <div className="flex flex-wrap items-center gap-2">
      {closed && (
        <Badge variant="outline" className="gap-1 border-main-accent-t3/40 text-main-accent-t3">
          Redeem only
        </Badge>
      )}
      {stale && (
        <Badge variant="outline" className="gap-1 border-main-accent-t3/40 text-main-accent-t3">
          <Clock className="size-3" /> Stale NAV
          <TipAnchor anchor="invest.position.stale-nav" />
        </Badge>
      )}
    </div>
  );
}
