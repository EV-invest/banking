"use client";

// "About this product" on `/invest/[service]` — the same facts the catalog card fronts,
// read in one place before the dealing panels below are opened: the strategy in a line,
// what backs the units, what the product charges, how money comes out, when it was last
// valued and how much supply is left. The supply and fee cards beside it hold the detail;
// this holds the sentence.
//
// No documents link yet: nothing on the wire or in config carries a whitepaper URL for a
// product (the registry has `summary` alone), and a link to a page that does not exist is
// worse than none. Once `docs_url` reaches `Allocation`, it belongs at the foot of this card.

import { useT } from "@evinvest/i18n/react";
import { TriangleAlert } from "lucide-react";

import { Card, CardContent } from "@evinvest/uikit";

import type { FeePolicy, FundNav } from "@/shared/contracts";
import type { Liquidity } from "@/views/invest/lib/catalog-card";
import type { Product } from "@/views/invest/lib/product";
import { Note } from "@/views/invest/ui/atoms";
import { FactRow, ProductFacts } from "@/views/invest/ui/product-facts";

export function AboutProduct({ product, policy, nav, liquidity, inKind }: { product: Product; policy: FeePolicy | null | undefined; nav: FundNav | null; liquidity: Liquidity | undefined; inKind: boolean }) {
  const t = useT();
  return (
    <Card className="h-fit">
      <CardContent className="space-y-4 py-6">
        <p className="text-sm font-semibold">{t("invest.about.title")}</p>
        {product.summary && <p className="text-sm leading-relaxed text-ink-soft">{product.summary}</p>}
        <dl className="space-y-2.5 border-t border-border pt-4 text-sm">
          <FactRow label={t("invest.facts.backing")}>{t(inKind ? "invest.facts.backingInKind" : "invest.facts.backingCash")}</FactRow>
        </dl>
        <ProductFacts policy={policy} nav={nav} liquidity={liquidity} extended className="space-y-2.5 text-sm" />
      </CardContent>
    </Card>
  );
}

/**
 * The risk statement, placed beside the returns rather than in the About card: the P&L
 * figure is where a reader forms an expectation, so this is where the caveat has to be
 * read. Generic and without a number on purpose — a figure here would be a forecast.
 */
export function RiskNote() {
  const t = useT();
  return (
    <Note tone="muted">
      <span className="flex gap-2">
        <TriangleAlert className="mt-0.5 size-3.5 shrink-0" aria-hidden />
        <span>
          <span className="font-semibold text-ink">{t("invest.about.riskTitle")}</span> {t("invest.about.risk")}
        </span>
      </span>
    </Note>
  );
}
