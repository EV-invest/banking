"use client";

// What stands behind a product's units, as a chip. `in_kind` is the exception worth a
// mark — `Redeem` is refused on it — so the registry row and card show only that and stay
// quiet on the common case; the issuance panel passes `verbose` and names both, beside
// the control that flips them.

import { useT } from "@evinvest/i18n/react";
import { Badge } from "@evinvest/uikit";

import type { AllocationBacking } from "@/shared/contracts/admin";
import { cn } from "@/shared/lib/cn";

// Catalogue keys, not finished prose: module scope holds what to say, and the chip
// resolves it against the reader's locale.
const LABEL: Record<AllocationBacking, string> = {
  cash: "admin.alloc.backing.cash",
  in_kind: "admin.alloc.backing.inKind",
};

const HINT: Record<AllocationBacking, string> = {
  cash: "admin.alloc.backing.cashHint",
  in_kind: "admin.alloc.backing.inKindHint",
};

// Amber, like `closed`: a fact about how the product exits, not a fault.
const TONE: Record<AllocationBacking, string> = {
  cash: "border-border text-muted-foreground",
  in_kind: "border-main-accent-t3/40 text-main-accent-t3",
};

export function BackingBadge({ backing, verbose, className }: { backing: AllocationBacking; verbose?: boolean; className?: string }) {
  const t = useT();
  if (!verbose && backing === "cash") return null;
  return (
    // i18n-max: 12 — a chip beside the state cell.
    <Badge variant="outline" className={cn("whitespace-nowrap", TONE[backing], className)} title={t(HINT[backing])}>
      {t(LABEL[backing])}
    </Badge>
  );
}

export const backingHintKey = (backing: AllocationBacking): string => HINT[backing];
