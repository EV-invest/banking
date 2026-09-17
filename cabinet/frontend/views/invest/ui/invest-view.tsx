"use client";

// `/invest` — the catalog: what the whole holding is worth, what is free to deploy, and
// one card per product. Headed "Invest" and not "Your fund shares": most readers arrive
// here holding nothing, and a product is chosen from this list (#384).
//
// Two rounds of layout have been thrown away here. First the dealing forms, which made
// the list read as a stack of forms. Then the wide table-ish rows that replaced them:
// they left the left half of every row empty while the figures crowded the right edge,
// and two summary cards stretched to a shared height they had no content for. The page
// is now a single dense band plus a grid — nothing is sized by anything other than what
// is in it.

import { useT } from "@evinvest/i18n/react";
import { Sparkles } from "lucide-react";
import { useMemo } from "react";

import { Card, CardContent, Skeleton } from "@evinvest/uikit";

import { allocationsResource, positionsResource, redemptionsResource } from "@/entities/fund/model/fund-resource";
import { walletResource } from "@/entities/wallet/model/wallet-resource";
import { useKycGate } from "@/features/kyc/model/use-kyc-gate";
import { errorMessage } from "@/shared/lib/api-client";
import { useResource } from "@/shared/lib/resource";
import { TipAnchor } from "@/shared/tips";
import { StaggerItem } from "@/shared/ui/motion";
import { SectionLabel, PageFrame } from "@/shared/ui/page-frame";
import { ResourceError } from "@/shared/ui/resource-error";
import { isZero, toBaseUnits } from "@/views/invest/lib/format";
import { buildProducts, type Product } from "@/views/invest/lib/product";
import { PortfolioBand } from "@/views/invest/ui/portfolio-band";
import { ProductCard } from "@/views/invest/ui/product-card";

export function InvestView() {
  const t = useT();
  const positionList = useResource(positionsResource);
  const catalogList = useResource(allocationsResource);
  const redemptionList = useResource(redemptionsResource);
  const wallet = useResource(walletResource);
  // The tier from `/kyc/status` first, the profile's mirror second, and no verdict from a
  // failed read — the wallet's rule (`features/kyc/lib/money-gate`), reused: a tier-0
  // caller cannot fund a subscription, so the card's CTA sends them to verify instead.
  const { gated } = useKycGate();

  const positions = positionList.data?.positions ?? null;
  const catalog = catalogList.data?.allocations ?? null;
  const redemptions = redemptionList.data?.redemptions ?? [];
  // The free balance is context, not the subject of this screen — a failure to read it
  // hides the figure rather than failing the page.
  const available = wallet.data?.balance?.available ?? null;
  // Only a positions read that has never succeeded is worth an alert; a failed refresh over
  // holdings already on screen is not something to interrupt the page for.
  const error = positionList.data ? null : positionList.error ? errorMessage(positionList.error, t) : null;

  const products = useMemo<Product[] | null>(() => (catalog && positions ? buildProducts(catalog, positions) : null), [catalog, positions]);

  const held = products?.filter((p) => p.position && !isZero(p.position.units)) ?? [];
  const totals = held.reduce(
    (acc, p) => ({ value: acc.value + toBaseUnits(p.position?.value), cost: acc.cost + toBaseUnits(p.position?.cost_basis) }),
    { value: 0n, cost: 0n },
  );
  const queued = redemptions.filter((r) => r.state === "queued");

  return (
    <PageFrame title={t("invest.title")} width="content">
      {/* `invest.overview` is a SECTION tip — a descriptor block, not an inline ⓘ — so
          it cannot live inside the heading row: it laid a full-width bordered box across
          the title. It belongs under the header, which is also the one place this
          explanation should live (the hand-written subtitle that used to sit here said
          the same thing in slightly different words). */}
      <StaggerItem>
        <TipAnchor anchor="invest.overview" />
      </StaggerItem>

      {error && <ResourceError variant="alert" title={t("err.positionsLoad")} message={error} />}

      {!products ? (
        <StaggerItem className="space-y-4">
          <Skeleton className="h-28 w-full" />
          <div className="grid gap-4 lg:grid-cols-2">
            <Skeleton className="h-96 w-full" />
            <Skeleton className="h-96 w-full" />
          </div>
        </StaggerItem>
      ) : (
        <>
          <PortfolioBand invested={totals.value} cost={totals.cost} funds={held.length} available={available} queued={queued.length} />

          <StaggerItem as="section" className="space-y-3">
            <SectionLabel className="flex items-center gap-2">
              {t("invest.products")}
              {products.length > 0 && <span className="rounded-full bg-primary/15 px-2 py-0.5 text-xs font-semibold text-primary-ink">{products.length}</span>}
            </SectionLabel>
            {products.length === 0 ? (
              <Card>
                <CardContent className="flex flex-col items-center gap-2 py-16 text-center text-ink-soft">
                  <Sparkles className="size-6" />
                  <p className="text-sm">{t("invest.noFunds")}</p>
                  <p className="max-w-sm text-xs">{t("invest.noFundsHint")}</p>
                </CardContent>
              </Card>
            ) : (
              <div className="grid gap-4 lg:grid-cols-2">
                {products.map((product) => (
                  <ProductCard key={product.service} product={product} gated={gated} />
                ))}
              </div>
            )}
          </StaggerItem>
        </>
      )}
    </PageFrame>
  );
}
