"use client";

// One product on `/invest`, as a card that explains itself: what it is (`summary`), what
// backs it, what it charges, how money comes back out, what it is worth and since when,
// and how full it is — then the one action this caller can actually take (#384).
//
// A card rather than a row because a row at this width left the product's name alone on
// the left with the figures pushed against the right edge. It carries its own NAV, fee and
// book reads: the catalog RPC is presentation-only, and all three are cached per fund and
// shared with the product page, so opening a product from here opens on what the card
// already showed.

import { useLocale, useT } from "@evinvest/i18n/react";
import { TrendingUp } from "lucide-react";

import { Badge, Card, CardContent } from "@evinvest/uikit";

import { bookPolicyResource } from "@/entities/book/model/book-resource";
import { feePolicyResource, fundNavResource } from "@/entities/fund/model/fund-resource";
import { cn } from "@/shared/lib/cn";
import { useResource } from "@/shared/lib/resource";
import { ProductIcon, productTone } from "@/shared/ui/icons/products";
import { cardCta, liquidity } from "@/views/invest/lib/catalog-card";
import { formatSignedUsdt, formatUnits, formatUsdt, isNegative, isZero } from "@/views/invest/lib/format";
import { isClosed, isInKind, isLocked, type Product } from "@/views/invest/lib/product";
import { SupplyBar } from "@/views/invest/ui/atoms";
import { BackingBadge } from "@/views/invest/ui/backing-badge";
import { ProductCta } from "@/views/invest/ui/product-cta";
import { MarkDate, ProductFacts } from "@/views/invest/ui/product-facts";

export function ProductCard({ product, gated }: { product: Product; gated: boolean }) {
  const t = useT();
  const locale = useLocale();
  const nav = useResource(fundNavResource, product.service).data ?? null;
  const policy = useResource(feePolicyResource, product.service).data ?? null;
  const bookOpen = useResource(bookPolicyResource, product.service).data?.book_open ?? false;

  const held = product.position && !isZero(product.position.units) ? product.position : null;
  const closed = isClosed(product);
  const locked = isLocked(product);
  const loss = held ? isNegative(held.pnl) : false;
  const flat = held ? isZero(held.pnl) : true;
  const cta = cardCta({ product, nav, bookOpen, gated });

  return (
    <Card className="transition-colors hover:border-primary/40">
      <CardContent className="flex h-full flex-col gap-4 py-5">
        <div className="flex items-center gap-3">
          {/* Tinted by service id, not fixed to t1: the card and the rail's row are the
              same fund seen twice, and a single accent for every card made a catalog of
              them read as one block. */}
          <span className={cn("flex size-9 shrink-0 items-center justify-center rounded-lg", productTone(product.service))}>
            <ProductIcon icon={product.icon} className="size-4.5" />
          </span>
          <p className="min-w-0 flex-1 truncate text-base font-semibold">{product.title}</p>
          {/* A non-shrinking sibling of the `min-w-0 flex-1` title column, so a long badge
              is taken straight out of the fund's name. i18n-max: 12. */}
          <Badge variant="outline" className={cn(closed ? "border-accent-warn/40 text-accent-warn" : locked ? "border-border text-ink-soft" : "border-positive/40 text-positive")}>
            {closed ? t("invest.badge.redeemOnly") : locked ? t("invest.badge.locked") : t("invest.badge.open")}
          </Badge>
        </div>

        {/* The strategy in a line, and what stands behind it. A product registered without
            a summary shows only the chip rather than a blank line held open for one. */}
        <div className="space-y-2">
          {product.summary && <p className="line-clamp-2 text-sm text-ink-soft">{product.summary}</p>}
          <BackingBadge inKind={isInKind(product)} />
        </div>

        <div className="flex flex-wrap gap-x-6 gap-y-3 border-y border-border py-3.5">
          <CardStat label={t("invest.navPerUnit")} value={nav ? formatUsdt(nav.nav, locale) : "—"} large>
            <MarkDate nav={nav} />
          </CardStat>
          {held ? (
            <>
              <CardStat label={t("invest.yourUnits")} value={formatUnits(held.units, locale)} />
              <CardStat label={t("invest.value")} value={formatUsdt(held.value, locale)} />
              <CardStat
                label={t("invest.pnl")}
                value={flat ? "0.00" : formatSignedUsdt(held.pnl, locale)}
                tone={flat ? undefined : loss ? "text-accent-error" : "text-positive"}
                icon={flat ? undefined : <TrendingUp className={cn("size-3.5", loss && "rotate-180")} />}
              />
            </>
          ) : (
            <div className="ml-auto space-y-1 text-right">
              <p className="text-xs text-ink-soft">{t("invest.yourPosition")}</p>
              <p className="text-sm text-ink-soft">{t("invest.notInvestedYet")}</p>
            </div>
          )}
        </div>

        <ProductFacts policy={policy} nav={nav} liquidity={liquidity(product, bookOpen)} className="space-y-1.5 text-xs" />

        {nav && <SupplyBar issued={nav.units_outstanding} cap={nav.unit_cap} />}

        <ProductCta cta={cta} service={product.service} title={product.title} />
      </CardContent>
    </Card>
  );
}

function CardStat({ label, value, large, tone, icon, children }: { label: string; value: string; large?: boolean; tone?: string; icon?: React.ReactNode; children?: React.ReactNode }) {
  return (
    <div className="space-y-1">
      <p className="text-xs text-ink-soft">{label}</p>
      <p className={cn("flex items-center gap-1 font-semibold tabular-nums", large ? "text-xl" : "text-sm", tone)}>
        {icon}
        {value}
      </p>
      {children && <p className="text-xs text-ink-soft">{children}</p>}
    </div>
  );
}
