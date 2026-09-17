"use client";

// The header of `/invest/[service]`: the product's mark, name, state and the two dealing
// buttons. Its own file so the page stays the composition — which reads land, what the
// product is to this caller — and this stays the strip that says so.

import { useT } from "@evinvest/i18n/react";
import { ArrowDownToLine, Sparkles } from "lucide-react";

import { Button } from "@evinvest/uikit";

import type { FundNav } from "@/shared/contracts";
import { cn } from "@/shared/lib/cn";
import { ProductIcon, productTone } from "@/shared/ui/icons/products";
import { StaggerItem } from "@/shared/ui/motion";
import { blockedReasonKey, isClosed, isInKind, isLocked, type Product } from "@/views/invest/lib/product";
import { ProductBadges, TEAL_CTA } from "@/views/invest/ui/atoms";
import { InKindBadge } from "@/views/invest/ui/backing-badge";
import { TradeLink } from "@/views/invest/ui/trade-link";

export type Panel = "subscribe" | "redeem" | null;

export function ProductHeader({ product, nav, held, panel, onPanel }: { product: Product; nav: FundNav | null; held: boolean; panel: Panel; onPanel: (next: (p: Panel) => Panel) => void }) {
  const t = useT();
  const closed = isClosed(product);
  const stale = nav?.stale ?? false;
  const blocked = blockedReasonKey(product, nav);
  return (
    <StaggerItem as="header" className="flex flex-wrap items-start justify-between gap-4">
      <div className="min-w-0 space-y-1">
        <div className="flex flex-wrap items-center gap-3">
          {/* The same mark and tint the rail and the invest card drew, so arriving here
              from either one lands on a header that is recognisably the row just clicked. */}
          <span className={cn("flex size-10 shrink-0 items-center justify-center rounded-xl", productTone(product.service))}>
            <ProductIcon icon={product.icon} className="size-5" />
          </span>
          <h1 className="text-3xl font-semibold">{product.title}</h1>
          <ProductBadges closed={closed} locked={isLocked(product)} stale={stale} />
          {isInKind(product) && <InKindBadge />}
        </div>
        {product.summary && <p className="max-w-xl pt-1 text-sm text-ink-soft">{product.summary}</p>}
      </div>
      <div className="flex flex-wrap gap-2">
        {/* The secondary market, beside the primary one: trading units with other holders
            is a different act from subscribing, priced by the book rather than the NAV. */}
        <TradeLink service={product.service} />
        {!closed && (
          <Button type="button" className={cn(TEAL_CTA)} disabled={blocked !== null} onClick={() => onPanel((p) => (p === "subscribe" ? null : "subscribe"))}>
            <Sparkles className="size-4" />
            {panel === "subscribe" ? t("ui.close") : t("invest.subscribe")}
          </Button>
        )}
        {held && (
          <Button type="button" variant="outline" disabled={stale} onClick={() => onPanel((p) => (p === "redeem" ? null : "redeem"))}>
            <ArrowDownToLine className="size-4" />
            {panel === "redeem" ? t("ui.close") : t("invest.redeem")}
          </Button>
        )}
      </div>
    </StaggerItem>
  );
}
