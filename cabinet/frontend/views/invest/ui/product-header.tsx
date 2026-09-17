"use client";

// The header of `/invest/[service]`: the product's mark, name, state and the two dealing
// buttons. Its own file so the page stays the composition — which reads land, what the
// product is to this caller — and this stays the strip that says so.

import { useT } from "@evinvest/i18n/react";
import { ArrowDownToLine, ShieldCheck, Sparkles } from "lucide-react";

import { Button } from "@evinvest/uikit";

import { useKycGate } from "@/features/kyc";
import type { FundNav } from "@/shared/contracts";
import { cn } from "@/shared/lib/cn";
import { Link } from "@/shared/ui/cabinet-link";
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
        {!closed && <SubscribeControl product={product} blocked={blocked !== null} panel={panel} onPanel={onPanel} />}
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

// The primary action, or — for a caller the hub has not cleared — the muted way to unlock it.
// Opening the panel for a tier-0 reader showed a form-sized block whose only content was the
// gate; the button now says the gate's way out itself, in the same words and to the same
// place as the catalog card's `verify` CTA (`views/invest/lib/catalog-card`). Same precedence
// as there: an operator's lock is named ahead of the caller's own tier.
//
// Until the tier is known the button stands disabled rather than absent: a teal Subscribe
// that vanished into a skeleton and came back would move the header on every cold load, and
// a click in that window cannot be honoured anyway.
function SubscribeControl({ product, blocked, panel, onPanel }: { product: Product; blocked: boolean; panel: Panel; onPanel: (next: (p: Panel) => Panel) => void }) {
  const t = useT();
  const { gated, loading } = useKycGate();
  if (gated && !isLocked(product)) {
    return (
      <Button asChild variant="outline">
        <Link href="/profile">
          <ShieldCheck className="size-4" />
          {t("invest.cta.verify")}
        </Link>
      </Button>
    );
  }
  return (
    <Button type="button" className={cn(TEAL_CTA)} disabled={blocked || loading} onClick={() => onPanel((p) => (p === "subscribe" ? null : "subscribe"))}>
      <Sparkles className="size-4" />
      {panel === "subscribe" ? t("ui.close") : t("invest.subscribe")}
    </Button>
  );
}
