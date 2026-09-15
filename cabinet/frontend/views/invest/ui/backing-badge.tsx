"use client";

// An in-kind product, marked where the holder decides what to do about it. The units
// stand for an asset held off-platform and the fund holds no cash for them, so the exit
// is the book, not `Redeem` — a fact about the product that has to be read before the
// redeem button is, which is why it sits in the header beside `closed` and `locked`
// rather than only on the refused form.

import { Package } from "lucide-react";

import { useT } from "@evinvest/i18n/react";
import { Badge } from "@evinvest/uikit";

import { Note } from "@/views/invest/ui/atoms";

export function InKindBadge() {
  const t = useT();
  return (
    // Inside the header's `flex-wrap` row, like `ProductBadges` — safe at any length.
    <Badge variant="outline" className="gap-1 border-main-accent-t3/40 text-main-accent-t3" title={t("invest.backing.inKindNote")}>
      <Package className="size-3" /> {t("invest.backing.inKind")}
    </Badge>
  );
}

/** The sentence behind the chip, stated once above the dealing panels. */
export function InKindNote() {
  const t = useT();
  return <Note tone="muted">{t("invest.backing.inKindNote")}</Note>;
}
