"use client";

// What stands behind a product's units, as a chip. `in_kind` is the exception worth a
// mark — `Redeem` is refused on it — so the registry row and card show only that and stay
// quiet on the common case; the issuance panel passes `verbose` and names both, beside
// the control that flips them.

import type { Translate } from "@evinvest/i18n";
import { useT } from "@evinvest/i18n/react";
import { Badge } from "@evinvest/uikit";

import type { AllocationBacking } from "@/shared/contracts/admin";
import { cn } from "@/shared/lib/cn";
import { wordFor } from "@/shared/lib/wire-words";

// Catalogue keys, not finished prose: module scope holds what to say, and the chip
// resolves it against the reader's locale.
const labels = (t: Translate): Record<AllocationBacking, string> => ({
  cash: t("admin.alloc.backing.cash", "Cash-backed"),
  in_kind: t("admin.alloc.backing.inKind", "In kind"),
});

const hints = (t: Translate): Record<AllocationBacking, string> => ({
  cash: t("admin.alloc.backing.cashHint", "Units were paid for into the fund's cash, and a redemption pays out of it at NAV."),
  in_kind: t("admin.alloc.backing.inKindHint", "Units stand for an asset the holders own off-platform. The fund holds no cash for them, so Redeem is refused and holders exit through the book."),
});

// Amber, like `closed`: a fact about how the product exits, not a fault.
const TONE: Record<AllocationBacking, string> = {
  cash: "border-border text-ink-soft",
  in_kind: "border-accent-warn/40 text-accent-warn",
};

export function BackingBadge({ backing, verbose, className }: { backing: AllocationBacking; verbose?: boolean; className?: string }) {
  const t = useT();
  if (!verbose && backing === "cash") return null;
  return (
    // i18n-max: 12 — a chip beside the state cell.
    <Badge variant="outline" className={cn("whitespace-nowrap", TONE[backing], className)} title={wordFor(hints(t), backing)}>
      {wordFor(labels(t), backing) ?? backing}
    </Badge>
  );
}

export const backingHint = (backing: AllocationBacking, t: Translate): string => wordFor(hints(t), backing) ?? backing;
