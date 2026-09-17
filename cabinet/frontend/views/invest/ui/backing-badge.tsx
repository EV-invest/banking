"use client";

// An in-kind product, marked where the holder decides what to do about it. The units
// stand for an asset held off-platform and the fund holds no cash for them, so the exit
// is the book, not `Redeem` — a fact about the product that has to be read before the
// redeem button is, which is why it sits in the header beside `closed` and `locked`
// rather than only on the refused form.

import { Banknote, Package } from "lucide-react";

import { useT } from "@evinvest/i18n/react";
import { Badge } from "@evinvest/uikit";

import { Note } from "@/views/invest/ui/atoms";

export function InKindBadge() {
  const t = useT();
  return (
    // Inside the header's `flex-wrap` row, like `ProductBadges` — safe at any length.
    <Badge variant="outline" className="gap-1 border-accent-warn/40 text-accent-warn" title={t("invest.backing.inKindNote")}>
      <Package className="size-3" /> {t("invest.backing.inKind")}
    </Badge>
  );
}

/**
 * What stands behind the units, on the catalog card — where `InKindBadge` alone left a
 * cash-backed product with no word about its backing, and so nothing to compare against.
 * The cash chip is quiet on purpose: it is the default, and the in-kind one is the warning.
 */
export function BackingBadge({ inKind }: { inKind: boolean }) {
  const t = useT();
  if (inKind) return <InKindBadge />;
  return (
    <Badge variant="outline" className="gap-1 border-border text-ink-soft">
      <Banknote className="size-3" /> {t("invest.backing.cash")}
    </Badge>
  );
}

/** The sentence behind the chip, stated once above the dealing panels. */
export function InKindNote() {
  const t = useT();
  return <Note tone="muted">{t("invest.backing.inKindNote")}</Note>;
}
