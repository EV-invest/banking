"use client";

// `/invest` — the portfolio view of the fund surface: what the whole holding is worth,
// what is free to deploy, and one row per product.
//
// This screen used to *be* the dealing surface — every fund carried its own subscribe and
// redeem forms inline, so a holder scrolled past four expandable panels to answer "how am
// I doing?". Dealing now lives on the product page (`/invest/[service]`); what is left
// here is a summary and a list, and each row's job is to be readable at a glance and to
// lead somewhere.

import { ArrowRight, Clock, Sparkles, TrendingUp, TriangleAlert, Wallet } from "lucide-react";
import Link from "next/link";
import { useCallback, useEffect, useMemo, useState } from "react";

import { Alert, AlertDescription, AlertTitle, Badge, Button, Card, CardContent, Skeleton } from "@evinvest/uikit";

import { fetchAllocations, fetchFundNav, fetchPositions, fetchRedemptions } from "@/entities/fund/api/fund-client";
import { fetchWallet } from "@/entities/wallet/api/wallet-client";
import type { Allocation, FundNav, Position, Redemption } from "@/shared/contracts";
import { cn } from "@/shared/lib/cn";
import { TipAnchor } from "@/shared/tips";
import { formatSignedUsdt, formatUnits, formatUsdt, fromBaseUnits, isNegative, isZero, toBaseUnits } from "@/views/invest/lib/format";
import { buildProducts, type Product } from "@/views/invest/lib/product";
import { SupplyBar, TEAL_CTA } from "@/views/invest/ui/atoms";

export function InvestView() {
  const [positions, setPositions] = useState<Position[] | null>(null);
  const [catalog, setCatalog] = useState<Allocation[] | null>(null);
  const [redemptions, setRedemptions] = useState<Redemption[]>([]);
  const [available, setAvailable] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    fetchPositions()
      .then((list) => {
        setPositions(list.positions ?? []);
        setError(null);
      })
      .catch((e: Error) => setError(e.message));
    fetchRedemptions()
      .then((list) => setRedemptions(list.redemptions ?? []))
      .catch(() => setRedemptions([]));
    // The free balance is context, not the subject of this screen — a failure to read it
    // hides the tile rather than failing the page.
    fetchWallet()
      .then((w) => setAvailable(w.balance?.available ?? "0"))
      .catch(() => setAvailable(null));
  }, []);

  useEffect(load, [load]);

  useEffect(() => {
    fetchAllocations()
      .then((list) => setCatalog(list.allocations ?? []))
      .catch(() => setCatalog([]));
  }, []);

  const products = useMemo<Product[] | null>(() => (catalog && positions ? buildProducts(catalog, positions) : null), [catalog, positions]);

  const held = products?.filter((p) => p.position && !isZero(p.position.units)) ?? [];
  const totals = held.reduce(
    (acc, p) => ({
      value: acc.value + toBaseUnits(p.position?.value),
      cost: acc.cost + toBaseUnits(p.position?.cost_basis),
    }),
    { value: 0n, cost: 0n },
  );
  const queued = redemptions.filter((r) => r.state === "queued");

  return (
    <div className="container max-w-5xl space-y-8 py-12">
      <header className="space-y-1">
        <p className="font-mono-tech text-xs uppercase tracking-widest text-main-accent-t1">Invest</p>
        <h1 className="flex items-center gap-2 text-3xl font-semibold">
          Your fund shares
          <TipAnchor anchor="invest.overview" />
        </h1>
        {/* The single most misread thing about this product: a holding does not grow in
            unit count. Say it on the screen, not only in a tooltip. */}
        <p className="text-sm text-muted-foreground">Your unit count stays put — it is the NAV per unit that moves, and with it what your units are worth.</p>
      </header>

      {error && (
        <Alert variant="destructive">
          <TriangleAlert className="size-4" />
          <AlertTitle>Couldn&apos;t load your positions</AlertTitle>
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}

      {!products ? (
        <div className="space-y-4">
          <Skeleton className="h-40 w-full" />
          <Skeleton className="h-28 w-full" />
          <Skeleton className="h-28 w-full" />
        </div>
      ) : (
        <>
          <Summary invested={totals.value} cost={totals.cost} funds={held.length} available={available} queued={queued} />

          <section className="space-y-3">
            <p className="flex items-center gap-2 text-xs font-semibold uppercase tracking-widest text-muted-foreground">
              Products
              {products.length > 0 && <span className="rounded-full bg-main-accent-t1/15 px-2 py-0.5 text-xs font-semibold text-main-accent-t1">{products.length}</span>}
            </p>
            {products.length === 0 ? (
              <Card>
                <CardContent className="flex flex-col items-center gap-2 py-16 text-center text-muted-foreground">
                  <Sparkles className="size-6" />
                  <p className="text-sm">No funds are open for subscription right now.</p>
                  <p className="max-w-sm text-xs">A fund appears here once an operator registers and opens it. Nothing you hold is affected.</p>
                </CardContent>
              </Card>
            ) : (
              <div className="space-y-3">
                {products.map((product) => (
                  <ProductRow key={product.service} product={product} />
                ))}
              </div>
            )}
          </section>
        </>
      )}
    </div>
  );
}

/** The one number this screen exists to answer, with the three that qualify it. */
function Summary({ invested, cost, funds, available, queued }: { invested: bigint; cost: bigint; funds: number; available: string | null; queued: Redemption[] }) {
  const pnl = invested - cost;
  const loss = pnl < 0n;
  const flat = pnl === 0n;
  // A percentage off a zero cost basis is not "0%", it is undefined — so it is omitted.
  const pct = cost > 0n ? Number((pnl * 10_000n) / cost) / 100 : null;

  return (
    <div className="grid gap-4 md:grid-cols-[minmax(0,2fr)_minmax(0,1fr)]">
      <Card>
        <CardContent className="space-y-4 py-6">
          <p className="text-xs font-semibold uppercase tracking-widest text-main-accent-t1">Invested value</p>
          <div className="flex flex-wrap items-baseline gap-3">
            <p className="text-4xl font-semibold tabular-nums">{formatUsdt(fromBaseUnits(invested))}</p>
            <span className="text-sm text-muted-foreground">USDT</span>
            {!flat && (
              <span className={cn("rounded-full px-2.5 py-1 text-xs font-semibold tabular-nums", loss ? "bg-main-accent-t4/15 text-main-accent-t4" : "bg-main-accent-t2/15 text-main-accent-t2")}>
                {formatSignedUsdt(fromBaseUnits(pnl))}
                {pct !== null && ` · ${pct > 0 ? "+" : ""}${pct.toFixed(2)}%`}
              </span>
            )}
          </div>
          <div className="grid gap-4 border-t border-border pt-4 sm:grid-cols-3">
            <MiniStat label="Cost basis" value={`${formatUsdt(fromBaseUnits(cost))} USDT`} />
            <MiniStat label="Funds held" value={String(funds)} />
            <MiniStat label="Awaiting settlement" value={queued.length === 0 ? "None" : `${queued.length} redemption${queued.length === 1 ? "" : "s"}`} tone={queued.length > 0 ? "text-main-accent-t3" : undefined} />
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardContent className="flex h-full flex-col justify-between gap-4 py-6">
          <div className="space-y-1">
            <p className="flex items-center gap-1.5 text-sm text-muted-foreground">
              <Wallet className="size-3.5" /> Available to invest
            </p>
            <p className="text-2xl font-semibold tabular-nums">{available === null ? "—" : `${formatUsdt(available)} USDT`}</p>
          </div>
          <Button asChild type="button" variant="outline" className="w-full">
            <Link href="/wallet">Top up</Link>
          </Button>
        </CardContent>
      </Card>
    </div>
  );
}

function MiniStat({ label, value, tone }: { label: string; value: string; tone?: string }) {
  return (
    <div className="space-y-1">
      <p className="text-xs text-muted-foreground">{label}</p>
      <p className={cn("text-sm font-semibold tabular-nums", tone)}>{value}</p>
    </div>
  );
}

/**
 * One product, as a link.
 *
 * The row carries its own NAV read: the catalog RPC is presentation-only, and a price is
 * the thing a holder is actually scanning this list for.
 */
function ProductRow({ product }: { product: Product }) {
  const [nav, setNav] = useState<FundNav | null>(null);

  useEffect(() => {
    let active = true;
    fetchFundNav(product.service)
      .then((n) => active && setNav(n))
      .catch(() => active && setNav(null));
    return () => {
      active = false;
    };
  }, [product.service]);

  const held = product.position && !isZero(product.position.units) ? product.position : null;
  const closed = product.allocation === null;
  const loss = held ? isNegative(held.pnl) : false;
  const flat = held ? isZero(held.pnl) : true;

  return (
    <Card className="transition-colors hover:border-main-accent-t1/40">
      <CardContent className="py-5">
        <Link href={`/invest/${encodeURIComponent(product.service)}`} className="flex flex-col gap-4 lg:flex-row lg:items-center">
          <div className="min-w-0 flex-1 space-y-2">
            <div className="flex flex-wrap items-center gap-2">
              <span className="text-base font-semibold">{product.title}</span>
              {closed && (
                <Badge variant="outline" className="border-main-accent-t3/40 text-main-accent-t3">
                  Redeem only
                </Badge>
              )}
              {nav?.stale && (
                <Badge variant="outline" className="gap-1 border-main-accent-t3/40 text-main-accent-t3">
                  <Clock className="size-3" /> Stale NAV
                </Badge>
              )}
              <span className="font-mono-tech text-xs text-muted-foreground">{product.service}</span>
            </div>
            {product.summary && <p className="max-w-lg text-sm text-muted-foreground">{product.summary}</p>}
            {nav && <SupplyBar issued={nav.units_outstanding} cap={nav.unit_cap} className="max-w-md pt-1" />}
          </div>

          <div className="flex flex-wrap items-center gap-x-8 gap-y-3">
            <RowStat label="NAV / unit" value={nav ? formatUsdt(nav.nav) : "—"} />
            <RowStat label="Your units" value={held ? formatUnits(held.units) : "—"} />
            <RowStat label="Value" value={held ? `${formatUsdt(held.value)}` : "—"} />
            <RowStat
              label="P&L"
              value={held && !flat ? formatSignedUsdt(held.pnl) : held ? "0.00" : "—"}
              tone={held && !flat ? (loss ? "text-main-accent-t4" : "text-main-accent-t2") : undefined}
              icon={held && !flat ? <TrendingUp className={cn("size-3.5", loss && "rotate-180")} /> : undefined}
            />
            <span className={cn("inline-flex items-center gap-1.5 rounded-md px-3 py-2 text-sm font-semibold", held ? "text-main-accent-t1" : TEAL_CTA)}>
              {held ? "Manage" : "Invest"}
              <ArrowRight className="size-4" />
            </span>
          </div>
        </Link>
      </CardContent>
    </Card>
  );
}

function RowStat({ label, value, tone, icon }: { label: string; value: string; tone?: string; icon?: React.ReactNode }) {
  return (
    <div className="min-w-20 space-y-1 lg:text-right">
      <p className="text-xs text-muted-foreground">{label}</p>
      <p className={cn("flex items-center gap-1 text-sm font-semibold tabular-nums lg:justify-end", tone)}>
        {icon}
        {value}
      </p>
    </div>
  );
}
