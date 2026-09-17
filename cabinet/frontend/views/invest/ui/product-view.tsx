"use client";

// `/invest/[service]` — one product, in full.
//
// The list answers "how am I doing?"; this answers "what is this, and what do I want to
// do about it?". Dealing lives here rather than on the list because a subscription is a
// decision about one fund, and a form that appears under a row you happened to expand
// makes it look like a detail of the row rather than the point of the page.
//
// A slug that is not a registered allocation is a 404 from the hub — deliberately, since
// a page that rendered an unregistered service would be the same phantom-fund surface the
// registry exists to close. The product is read from the hub's detail route and not from
// the open catalog, because a `hidden` product this caller was granted is not listed
// (`selectProduct`).

import { useT } from "@evinvest/i18n/react";
import { TriangleAlert } from "lucide-react";
import { useState } from "react";

import { Alert, AlertDescription, AlertTitle } from "@evinvest/uikit";

import { bookPolicyResource } from "@/entities/book/model/book-resource";
import { accruedFeesResource, allocationDetailResource, allocationsResource, feePolicyResource, fundNavResource, positionsResource, redemptionsResource } from "@/entities/fund/model/fund-resource";
import { errorMessage, RequestError } from "@/shared/lib/api-client";
import { useResource } from "@/shared/lib/resource";
import { SECTION_STAGGER, Stagger, StaggerItem } from "@/shared/ui/motion";
import { liquidity } from "@/views/invest/lib/catalog-card";
import { isZero } from "@/views/invest/lib/format";
import { blockedReasonKey, isClosed, isInKind, selectProduct } from "@/views/invest/lib/product";
import { AboutProduct, RiskNote } from "@/views/invest/ui/about-product";
import { Note } from "@/views/invest/ui/atoms";
import { InKindNote } from "@/views/invest/ui/backing-badge";
import { QueuedList, RedeemPanel, SubscribePanel } from "@/views/invest/ui/deal-panels";
import { FeeCard, SupplyCard } from "@/views/invest/ui/product-cards";
import { type Panel, ProductHeader } from "@/views/invest/ui/product-header";
import { BackLink, HoldingStats, PriceOnly, ProductLoading, ProductMissing } from "@/views/invest/ui/product-stats";

export function ProductView({ service }: { service: string }) {
  const t = useT();
  const [panel, setPanel] = useState<Panel>(null);

  // The detail and the positions decide what this product *is* to this caller:
  // open-and-unheld, open-and-held, closed-but-still-held, or hidden-but-granted. The
  // catalog and the positions were already read by the list this page is usually entered
  // from, so the product paints on the first frame while the detail confirms it.
  const detailRead = useResource(allocationDetailResource, service);
  const catalogList = useResource(allocationsResource);
  const positionList = useResource(positionsResource);
  const navRead = useResource(fundNavResource, service);
  const redemptionList = useResource(redemptionsResource);
  // Two separate reads on purpose: the terms are the fund's and cache for minutes, while
  // the accrued figure is the caller's own and moves every second the clock runs.
  const feeRead = useResource(feePolicyResource, service);
  const accruedRead = useResource(accruedFeesResource, service);
  // Read here as well as in `TradeLink`: the liquidity line in "About" says whether the
  // book is a way out, and the same cached read answers both.
  const bookRead = useResource(bookPolicyResource, service);
  const bookOpen = bookRead.isLoading ? undefined : (bookRead.data?.book_open ?? false);

  // A 404 is an answer ("not registered", or not for this caller), not a failed read —
  // it gets the not-found copy below, never the transport's generic sentence.
  const detailFailed = !detailRead.data && detailRead.error && !(detailRead.error instanceof RequestError && detailRead.error.status === 404) ? detailRead.error : null;
  const readFailed = detailFailed || (!catalogList.data && catalogList.error) || (!positionList.data && positionList.error);
  const error = readFailed ? errorMessage(readFailed, t) : null;
  const product = selectProduct(service, {
    detail: detailRead,
    catalog: catalogList.data?.allocations,
    positions: { data: positionList.data?.positions, isLoading: positionList.isLoading },
  });

  const nav = navRead.data ?? null;
  const feePolicy = feeRead.data ?? null;
  // Unread is not "no terms": the About block waits rather than saying so (`FeeHeadline`).
  const aboutPolicy = feeRead.isLoading ? undefined : feePolicy;
  const accruedFees = accruedRead.data ?? null;
  const redemptions = (redemptionList.data?.redemptions ?? []).filter((r) => r.service === service);

  if (product === undefined) return <ProductLoading />;
  if (product === null) return <ProductMissing service={service} error={error} />;

  const held = product.position && !isZero(product.position.units) ? product.position : null;
  const closed = isClosed(product);
  const inKind = isInKind(product);
  // `posted_at` is 0 until an operator marks the fund, which is exactly when the hub is
  // still pricing at the bootstrap NAV of 1.0.
  const unmarked = nav !== null && Number(nav.posted_at ?? 0) === 0;
  const blocked = blockedReasonKey(product, nav);
  const queued = redemptions.filter((r) => r.state === "queued");

  return (
    <Stagger step={SECTION_STAGGER} className="container max-w-4xl space-y-7 py-12">
      <StaggerItem>
        <BackLink />
      </StaggerItem>

      <ProductHeader product={product} nav={nav} held={held !== null} panel={panel} onPanel={setPanel} />

      {error && (
        <StaggerItem>
          <Alert variant="destructive">
            <TriangleAlert className="size-4" />
            <AlertTitle>{t("err.fundRefresh")}</AlertTitle>
            <AlertDescription>{error}</AlertDescription>
          </Alert>
        </StaggerItem>
      )}

      {/* The two columns are one section. The dealing panels inside open and close under
          `Panel`, and a stagger that also owned them would be animating the same elements
          from two directions the first time one is opened. */}
      <StaggerItem className="grid gap-5 lg:grid-cols-[minmax(0,2fr)_minmax(0,1fr)]">
        <div className="space-y-5">
          {held ? <HoldingStats position={held} /> : <PriceOnly nav={nav} unmarked={unmarked} />}
          <RiskNote />

          {/* The gates, stated before the action rather than after a failed submit. */}
          {blocked && <Note tone="amber">{t(blocked)}</Note>}
          {unmarked && !closed && <Note tone="muted">{t("invest.unmarkedNote")}</Note>}
          {inKind && <InKindNote />}

          {panel === "subscribe" && !blocked && <SubscribePanel service={product.service} nav={nav} />}
          {/* An in-kind product still opens the panel: the refusal is explained on the
              form, with the way out beside it, rather than met as a 412 after the click. */}
          {panel === "redeem" && held && <RedeemPanel service={product.service} position={held} nav={nav} inKind={inKind} />}

          {queued.length > 0 && <QueuedList items={queued} />}
        </div>

        <div className="space-y-5">
          <AboutProduct product={product} policy={aboutPolicy} nav={nav} liquidity={liquidity(product, bookOpen)} inKind={inKind} />
          <SupplyCard nav={nav} />
          <FeeCard policy={feePolicy} accrued={held ? accruedFees : null} />
        </div>
      </StaggerItem>
    </Stagger>
  );
}
