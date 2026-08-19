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
// registry exists to close.

import { ArrowDownToLine, ArrowLeft, Loader2, Sparkles, TrendingUp, TriangleAlert } from "lucide-react";
import Link from "next/link";
import { useState } from "react";

import { Alert, AlertDescription, AlertTitle, Button, Card, CardContent, Skeleton } from "@evinvest/uikit";

import { accruedFeesResource, allocationsResource, feePolicyResource, fundNavResource, positionsResource, redemptionsResource } from "@/entities/fund/model/fund-resource";
import type { AccruedFees, FeePolicy, FundNav, Position } from "@/shared/contracts";
import { cn } from "@/shared/lib/cn";
import { useResource } from "@/shared/lib/resource";
import { TipAnchor } from "@/shared/tips";
import { compactUnits, formatSignedUsdt, formatUnits, formatUsdt, isNegative, isZero } from "@/views/invest/lib/format";
import { blockedReason, buildProducts, type Product } from "@/views/invest/lib/product";
import { Note, ProductBadges, Stat, SupplyBar, TEAL_CTA } from "@/views/invest/ui/atoms";
import { QueuedList, RedeemPanel, SubscribePanel } from "@/views/invest/ui/deal-panels";

type Panel = "subscribe" | "redeem" | null;

export function ProductView({ service }: { service: string }) {
  const [panel, setPanel] = useState<Panel>(null);

  // The catalog and the positions are both needed to decide what this product *is* to this
  // caller: open-and-unheld, open-and-held, or closed-but-still-held. Both are cached and
  // both were already read by the list this page is usually entered from, so the product
  // resolves on the first frame. `undefined` is still "loading" and `null` is "no such
  // product" — collapsing the two would show a not-found flash on every cold load.
  const catalogList = useResource(allocationsResource);
  const positionList = useResource(positionsResource);
  const navRead = useResource(fundNavResource, service);
  const redemptionList = useResource(redemptionsResource);
  // Two separate reads on purpose: the terms are the fund's and cache for minutes, while
  // the accrued figure is the caller's own and moves every second the clock runs.
  const feeRead = useResource(feePolicyResource, service);
  const accruedRead = useResource(accruedFeesResource, service);

  const resolving = catalogList.isLoading || positionList.isLoading;
  const readFailed = (!catalogList.data && catalogList.error) || (!positionList.data && positionList.error);
  const error = readFailed ? readFailed.message : null;
  const product: Product | null | undefined = resolving
    ? undefined
    : (buildProducts(catalogList.data?.allocations ?? [], positionList.data?.positions ?? []).find((p) => p.service === service) ?? null);

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
            <p className="text-sm">{error ?? `No fund is registered as “${service}”.`}</p>
            <p className="max-w-sm text-xs">A fund exists only once an operator registers it. If you followed a link here, the product may never have been opened.</p>
          </CardContent>
        </Card>
      </div>
    );
  }

  const held = product.position && !isZero(product.position.units) ? product.position : null;
  const closed = product.allocation === null;
  const stale = nav?.stale ?? false;
  // `posted_at` is 0 until an operator marks the fund, which is exactly when the hub is
  // still pricing at the bootstrap NAV of 1.0.
  const unmarked = nav !== null && Number(nav.posted_at ?? 0) === 0;
  const blocked = blockedReason(product, nav);
  const queued = redemptions.filter((r) => r.state === "queued");

  return (
    <div className="container max-w-4xl space-y-7 py-12">
      <BackLink />

      <header className="flex flex-wrap items-start justify-between gap-4">
        <div className="min-w-0 space-y-1">
          <div className="flex flex-wrap items-center gap-3">
            <h1 className="text-3xl font-semibold">{product.title}</h1>
            <ProductBadges closed={closed} stale={stale} />
          </div>
          <p className="font-mono-tech text-xs text-muted-foreground">{product.service}</p>
          {product.summary && <p className="max-w-xl pt-1 text-sm text-muted-foreground">{product.summary}</p>}
        </div>
        <div className="flex flex-wrap gap-2">
          {!closed && (
            <Button type="button" className={cn(TEAL_CTA)} disabled={blocked !== null} onClick={() => setPanel((p) => (p === "subscribe" ? null : "subscribe"))}>
              <Sparkles className="size-4" />
              {panel === "subscribe" ? "Close" : "Subscribe"}
            </Button>
          )}
          {held && (
            <Button type="button" variant="outline" disabled={stale} onClick={() => setPanel((p) => (p === "redeem" ? null : "redeem"))}>
              <ArrowDownToLine className="size-4" />
              {panel === "redeem" ? "Close" : "Redeem"}
            </Button>
          )}
        </div>
      </header>

      {error && (
        <Alert variant="destructive">
          <TriangleAlert className="size-4" />
          <AlertTitle>Couldn&apos;t refresh this fund</AlertTitle>
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}

      <div className="grid gap-5 lg:grid-cols-[minmax(0,2fr)_minmax(0,1fr)]">
        <div className="space-y-5">
          {held ? <HoldingStats position={held} /> : <PriceOnly nav={nav} unmarked={unmarked} />}

          {/* The gates, stated before the action rather than after a failed submit. */}
          {blocked && <Note tone="amber">{blocked}</Note>}
          {unmarked && !closed && <Note tone="muted">No valuation posted yet — units price at the bootstrap NAV of 1.0 until the first mark.</Note>}

          {panel === "subscribe" && !blocked && <SubscribePanel service={product.service} nav={nav} />}
          {panel === "redeem" && held && <RedeemPanel service={product.service} position={held} nav={nav} />}

          {queued.length > 0 && <QueuedList items={queued} />}
        </div>

        <div className="space-y-5">
          <SupplyCard nav={nav} />
          <FeeCard policy={feePolicy} accrued={held ? accruedFees : null} />
        </div>
      </div>
    </div>
  );
}

function BackLink() {
  return (
    <Link href="/invest" className="inline-flex items-center gap-1.5 text-sm text-muted-foreground transition-colors hover:text-foreground">
      <ArrowLeft className="size-4" />
      All products
    </Link>
  );
}

/**
 * The product's supply, as a fact about the fund rather than about the holder.
 *
 * This is the visible half of the unit cap: a fund is sized by an operator, and once the
 * authorised units are issued it stops minting. Saying so here means "subscription
 * refused" is never the first time a holder hears about it.
 */
function SupplyCard({ nav }: { nav: FundNav | null }) {
  if (!nav) {
    return (
      <Card>
        <CardContent className="py-6">
          <Loader2 className="size-4 animate-spin text-muted-foreground" />
        </CardContent>
      </Card>
    );
  }
  return (
    <Card className="h-fit">
      <CardContent className="space-y-4 py-6">
        <p className="text-sm font-semibold">Unit supply</p>
        <SupplyBar issued={nav.units_outstanding} cap={nav.unit_cap} />
        <dl className="space-y-2.5 border-t border-border pt-4 text-sm">
          <Row label="Remaining capacity" value={`${compactUnits(nav.remaining_capacity)} units`} />
          <Row label="NAV / unit" value={`${formatUsdt(nav.nav)} USDT`} />
          <Row label="Fund AUM" value={nav.aum ? `${formatUsdt(nav.aum)} USDT` : "Not yet valued"} />
        </dl>
        <p className="text-xs text-muted-foreground">Subscriptions mint units at the current NAV. Once the cap is reached the fund stops accepting new money; redemptions are unaffected.</p>
      </CardContent>
    </Card>
  );
}

/**
 * What this fund charges, and what the caller's own holding has run up against it.
 *
 * It sits beside the supply card rather than inside the holding block because the terms
 * apply to a product whether or not the caller is in it — somebody deciding whether to
 * subscribe needs to read the fee before they act, not discover it on the first charge.
 *
 * A product with no policy renders nothing at all. That is deliberate: an absent policy
 * and a policy of zeros are different facts, and a card of zeros reads like a fee that was
 * generously waived rather than a fund that never had one.
 *
 * The accrued figures are shown only to a holder, because they are a statement about a
 * position. `total` is what would be taken if the fee were charged this instant — the
 * number the `Value` stat opposite has NOT been reduced by — so the card says so plainly
 * instead of leaving the two to be reconciled by the reader.
 */
function FeeCard({ policy, accrued }: { policy: FeePolicy | null; accrued: AccruedFees | null }) {
  if (!policy?.configured) return null;
  const owed = accrued?.configured ? accrued : null;
  return (
    <Card className="h-fit">
      <CardContent className="space-y-4 py-6">
        <p className="text-sm font-semibold">Fees</p>
        <dl className="space-y-2.5 text-sm">
          <Row label="Management" value={`${percent(policy.management_bps)} p.a.`} />
          <Row label="Performance" value={`${percent(policy.performance_bps)} of the gain`} />
          {policy.hurdle_bps ? <Row label="Hurdle" value={`${percent(policy.hurdle_bps)} p.a. first`} /> : null}
          <Row label="Charged on" value={BASIS_LABEL[policy.basis ?? ""] ?? policy.basis ?? "—"} />
          <Row label="Locked in" value={PERIOD_LABEL[policy.crystallization ?? ""] ?? policy.crystallization ?? "—"} />
        </dl>

        {owed && (
          <div className="space-y-2.5 border-t border-border pt-4">
            {/* Its own heading, so the rows can be labelled `Management` and `Performance`
                without colliding with the identically-named terms above. Long enough labels
                to disambiguate inline would wrap onto two lines in this column. */}
            <p className="text-xs uppercase tracking-wide text-muted-foreground">Accrued on your holding</p>
            <dl className="space-y-2.5 text-sm">
              <Row label="Management" value={`${formatUsdt(owed.management)} USDT`} />
              <Row label="Performance" value={`${formatUsdt(owed.performance)} USDT`} />
              {isZero(owed.debt) ? null : <Row label="Carried over" value={`${formatUsdt(owed.debt)} USDT`} />}
              <Row label="Total" value={`${formatUsdt(owed.total)} USDT`} />
              <Row label="Your mark" value={`${formatUsdt(owed.high_water_mark)} USDT`} />
            </dl>
          </div>
        )}

        <p className="text-xs text-muted-foreground">
          {owed
            ? "Fees are taken in units, never in cash — your wallet balance is untouched, and the value shown opposite is before them. The performance fee applies only to gains above your own high-water mark, so a recovery back to it costs nothing."
            : "Fees are taken in units, never in cash. The performance fee applies only to gains above the price you enter at, measured per investor rather than per fund."}
        </p>
      </CardContent>
    </Card>
  );
}

/** Basis points as an investor reads them: 200 → "2%", 250 → "2.5%". */
function percent(bps: number | undefined): string {
  const value = (bps ?? 0) / 100;
  return `${Number.isInteger(value) ? value : Number(value.toFixed(2))}%`;
}

const BASIS_LABEL: Record<string, string> = {
  invested_capital: "Invested capital",
  market_value: "Market value",
};

const PERIOD_LABEL: Record<string, string> = {
  monthly: "Monthly",
  quarterly: "Quarterly",
  semi_annual: "Every 6 months",
  annual: "Annually",
};

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-baseline justify-between gap-3">
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="font-medium tabular-nums">{value}</dd>
    </div>
  );
}

function HoldingStats({ position }: { position: Position }) {
  const loss = isNegative(position.pnl);
  const flat = isZero(position.pnl);
  return (
    <div className="grid gap-3 sm:grid-cols-2">
      <Stat label="Units" value={formatUnits(position.units)} tip="invest.position.units" />
      <Stat label="NAV" value={formatUsdt(position.nav)} tip="invest.position.nav" />
      <Stat label="Value" value={`${formatUsdt(position.value)} USDT`} emphasis tip="invest.position.value" />
      <Stat
        label="P&L"
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
  return (
    <Card>
      <CardContent className="flex flex-wrap items-center justify-between gap-4 py-6">
        <div className="space-y-1">
          <p className="flex items-center gap-1.5 text-xs uppercase tracking-wide text-muted-foreground">
            NAV / unit
            <TipAnchor anchor="invest.position.nav" />
          </p>
          <p className="text-2xl font-semibold tabular-nums">{nav ? `${formatUsdt(nav.nav)} USDT` : "—"}</p>
        </div>
        <p className="max-w-sm text-sm text-muted-foreground">{unmarked ? "Not yet valued — the first subscription prices at 1.0." : "You hold no units in this fund yet."}</p>
      </CardContent>
    </Card>
  );
}
