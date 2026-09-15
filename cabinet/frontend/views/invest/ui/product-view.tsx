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
import { ArrowDownToLine, ArrowLeft, Sparkles, TrendingUp, TriangleAlert } from "lucide-react";
import { Link } from "@/shared/ui/cabinet-link";
import { useState } from "react";

import { Alert, AlertDescription, AlertTitle, Button, Card, CardContent, Skeleton } from "@evinvest/uikit";

import { accruedFeesResource, allocationDetailResource, allocationsResource, feePolicyResource, fundNavResource, positionsResource, redemptionsResource } from "@/entities/fund/model/fund-resource";
import type { FundNav, Position } from "@/shared/contracts";
import { errorMessage, RequestError } from "@/shared/lib/api-client";
import { cn } from "@/shared/lib/cn";
import { useResource } from "@/shared/lib/resource";
import { TipAnchor } from "@/shared/tips";
import { ProductIcon, productTone } from "@/shared/ui/icons/products";
import { SECTION_STAGGER, Stagger, StaggerItem } from "@/shared/ui/motion";
import { formatSignedUsdt, formatUnits, formatUsdt, isNegative, isZero } from "@/views/invest/lib/format";
import { blockedReasonKey, isClosed, isInKind, isLocked, selectProduct } from "@/views/invest/lib/product";
import { Note, ProductBadges, Stat, TEAL_CTA } from "@/views/invest/ui/atoms";
import { InKindBadge, InKindNote } from "@/views/invest/ui/backing-badge";
import { QueuedList, RedeemPanel, SubscribePanel } from "@/views/invest/ui/deal-panels";
import { FeeCard, SupplyCard } from "@/views/invest/ui/product-cards";
import { TradeLink } from "@/views/invest/ui/trade-link";

type Panel = "subscribe" | "redeem" | null;

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
  const accruedFees = accruedRead.data ?? null;
  const redemptions = (redemptionList.data?.redemptions ?? []).filter((r) => r.service === service);

  if (product === undefined) {
    return (
      <div className="container max-w-4xl space-y-6 py-12">
        <Skeleton className="h-10 w-64" />
        <Skeleton className="h-40 w-full" />
        <Skeleton className="h-32 w-full" />
      </div>
    );
  }

  if (product === null) {
    return (
      <div className="container max-w-4xl space-y-6 py-12">
        <BackLink />
        <Card>
          <CardContent className="flex flex-col items-center gap-2 py-16 text-center text-muted-foreground">
            <TriangleAlert className="size-6" />
            <p className="text-sm">{error ?? t("invest.notRegistered", { service })}</p>
            <p className="max-w-sm text-xs">{t("invest.notRegisteredHint")}</p>
          </CardContent>
        </Card>
      </div>
    );
  }

  const held = product.position && !isZero(product.position.units) ? product.position : null;
  const closed = isClosed(product);
  const locked = isLocked(product);
  const inKind = isInKind(product);
  const stale = nav?.stale ?? false;
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

      <StaggerItem as="header" className="flex flex-wrap items-start justify-between gap-4">
        <div className="min-w-0 space-y-1">
          <div className="flex flex-wrap items-center gap-3">
            {/* The same mark and tint the rail and the invest card drew, so arriving here
                from either one lands on a header that is recognisably the row just clicked. */}
            <span className={cn("flex size-10 shrink-0 items-center justify-center rounded-xl", productTone(product.service))}>
              <ProductIcon icon={product.icon} className="size-5" />
            </span>
            <h1 className="text-3xl font-semibold">{product.title}</h1>
            <ProductBadges closed={closed} locked={locked} stale={stale} />
            {inKind && <InKindBadge />}
          </div>
          <p className="font-mono-tech text-xs text-muted-foreground">{product.service}</p>
          {product.summary && <p className="max-w-xl pt-1 text-sm text-muted-foreground">{product.summary}</p>}
        </div>
        <div className="flex flex-wrap gap-2">
          {/* The secondary market, beside the primary one: trading units with other holders
              is a different act from subscribing, priced by the book rather than the NAV. */}
          <TradeLink service={product.service} />
          {!closed && (
            <Button type="button" className={cn(TEAL_CTA)} disabled={blocked !== null} onClick={() => setPanel((p) => (p === "subscribe" ? null : "subscribe"))}>
              <Sparkles className="size-4" />
              {panel === "subscribe" ? t("ui.close") : t("invest.subscribe")}
            </Button>
          )}
          {held && (
            <Button type="button" variant="outline" disabled={stale} onClick={() => setPanel((p) => (p === "redeem" ? null : "redeem"))}>
              <ArrowDownToLine className="size-4" />
              {panel === "redeem" ? t("ui.close") : t("invest.redeem")}
            </Button>
          )}
        </div>
      </StaggerItem>

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
          <SupplyCard nav={nav} />
          <FeeCard policy={feePolicy} accrued={held ? accruedFees : null} />
        </div>
      </StaggerItem>
    </Stagger>
  );
}

function BackLink() {
  const t = useT();
  return (
    <Link href="/invest" className="inline-flex items-center gap-1.5 text-sm text-muted-foreground transition-colors hover:text-foreground">
      <ArrowLeft className="size-4" />
      {t("invest.allProducts")}
    </Link>
  );
}

function HoldingStats({ position }: { position: Position }) {
  const t = useT();
  const loss = isNegative(position.pnl);
  const flat = isZero(position.pnl);
  return (
    <div className="grid gap-3 sm:grid-cols-2">
      <Stat label={t("invest.units")} value={formatUnits(position.units)} tip="invest.position.units" />
      <Stat label={t("invest.nav")} value={formatUsdt(position.nav)} tip="invest.position.nav" />
      <Stat label={t("invest.value")} value={`${formatUsdt(position.value)} USDT`} emphasis tip="invest.position.value" />
      <Stat
        label={t("invest.pnl")}
        value={`${formatSignedUsdt(position.pnl)} USDT`}
        tip="invest.position.pnl"
        emphasis
        tone={loss && !flat ? "text-main-accent-t4" : "text-main-accent-t2"}
        icon={<TrendingUp className={cn("size-3.5", loss && !flat && "rotate-180")} />}
      />
    </div>
  );
}

/** A product the caller holds nothing in: the price is all there is to show. */
function PriceOnly({ nav, unmarked }: { nav: FundNav | null; unmarked: boolean }) {
  const t = useT();
  return (
    <Card>
      <CardContent className="flex flex-wrap items-center justify-between gap-4 py-6">
        <div className="space-y-1">
          <p className="flex items-center gap-1.5 text-xs uppercase tracking-wide text-muted-foreground">
            {t("invest.navPerUnit")}
            <TipAnchor anchor="invest.position.nav" />
          </p>
          <p className="text-2xl font-semibold tabular-nums">{nav ? `${formatUsdt(nav.nav)} USDT` : "—"}</p>
        </div>
        <p className="max-w-sm text-sm text-muted-foreground">{t(unmarked ? "invest.notYetValuedHint" : "invest.noUnitsInFund")}</p>
      </CardContent>
    </Card>
  );
}
