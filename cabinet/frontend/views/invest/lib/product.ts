// The invest surface's shared vocabulary: what a "product" is on screen, and the exact
// arithmetic the two deal panels preview with.
//
// Everything here is pure and display-only. The hub re-derives every figure server-side;
// these exist so a holder sees what they are about to get *before* submitting, in the
// same rounding the ledger will actually apply.

import type { Allocation, AllocationIcon, FundNav, Position } from "@/shared/contracts";

// Relative and with the extension, like `views/admin/allocations/lib/pin-cap.ts`: the
// node test runner resolves no `@/` alias, and `./format` only re-exports this module.
import { toBaseUnits } from "../../../shared/lib/money.ts";

/** 10^18 — the base-unit scale every money and unit amount is carried in. */
const SCALE = 10n ** 18n;

/**
 * One row of the invest surface: a product, plus whatever the caller holds in it.
 *
 * The two used to be separate sections, so a holder saw their fund twice — once as a
 * position and once as a row in the subscribe form's dropdown.
 */
export interface Product {
  service: string;
  title: string;
  summary: string;
  /**
   * Carried through unresolved — `undefined` both for a catalog entry rehydrated from
   * sessionStorage before this field existed and for a product known only by a holding.
   * `ProductIcon` owns the fall back to `fund`, so this module stays pure data with no
   * React in it and no glyph in its bundle.
   */
  icon: AllocationIcon | undefined;
  /** Absent when the product has left the open catalog but units are still held. */
  allocation: Allocation | null;
  position: Position | null;
}

/**
 * The open catalog UNION the services already held.
 *
 * A closed product is absent from the catalog but its holders still have units in it —
 * dropping it here would hide real money, and the hub deliberately keeps redeeming it.
 */
export function buildProducts(catalog: Allocation[], positions: Position[]): Product[] {
  const byService = new Map<string, Product>();
  for (const a of catalog) byService.set(a.service, listed(a, null));
  for (const p of positions) {
    const service = p.service ?? "";
    if (!service) continue;
    const existing = byService.get(service);
    if (existing) existing.position = p;
    else byService.set(service, heldOnly(service, p));
  }
  return [...byService.values()].sort((a, b) => a.title.localeCompare(b.title));
}

const listed = (a: Allocation, position: Position | null): Product => ({ service: a.service, title: a.title, summary: a.summary, icon: a.icon, allocation: a, position });

/** A product known only by a holding: nothing here knows its title or icon, so it draws
 *  the same default the hub would have given it. */
const heldOnly = (service: string, position: Position): Product => ({ service, title: service, summary: "", icon: undefined, allocation: null, position });

/** What one product's page has read so far. The shape of `ResourceSnapshot`, narrowed to
 *  the fields the decision needs, so this stays React-free and testable. */
export interface ProductReads {
  /** `GET /api/allocations/detail` — the hub's answer for THIS caller, in any state. */
  detail: { data: Allocation | undefined; error: Error | null; isLoading: boolean };
  /** The open catalog, `undefined` until it has loaded. */
  catalog: Allocation[] | undefined;
  positions: { data: Position[] | undefined; isLoading: boolean };
}

/**
 * The product behind `/invest/[service]` and its terminal.
 *
 * The detail is the authority: the catalog is the OPEN list, and a `hidden` product an
 * operator granted this caller is not on it — resolving from the catalog alone locked
 * such a holder out of a form the hub would have accepted. The catalog still paints the
 * first frame while the detail is in flight, since a page entered from the list should
 * not skeleton for a product it was just looking at.
 *
 * `undefined` is still "loading" and `null` is "no such product" — collapsing the two
 * would flash the not-found state on every cold load. A 404 is the hub saying this
 * caller may not see the product, or that it was never registered; units held in it are
 * still real money, so a holder keeps a (closed) product rather than a not-found page.
 * Any other failure falls back to whatever the catalog and the positions know.
 */
export function selectProduct(service: string, reads: ProductReads): Product | null | undefined {
  if (reads.positions.isLoading) return undefined;
  const position = reads.positions.data?.find((p) => p.service === service) ?? null;
  const fromCatalog = reads.catalog?.find((a) => a.service === service) ?? null;
  if (reads.detail.data) return listed(reads.detail.data, position);
  if (reads.detail.isLoading) return fromCatalog ? listed(fromCatalog, position) : undefined;
  if (isNotFound(reads.detail.error)) return position ? heldOnly(service, position) : null;
  if (fromCatalog) return listed(fromCatalog, position);
  return position ? heldOnly(service, position) : null;
}

// `RequestError` carries `status`; read structurally, the way `shared/lib/resource.ts`
// reads a 403, so this module needs no transport import.
function isNotFound(error: Error | null): boolean {
  const status: unknown = error ? (error as { status?: unknown }).status : undefined;
  return status === 404;
}

/** Closed to new money: delisted (known only by a holding) or registered but not `open`.
 *  The detail read returns a closed product where the catalog would have dropped it, so
 *  the state is checked and not just the presence. */
export function isClosed(product: Product): boolean {
  return product.allocation === null || product.allocation.state !== "open";
}

/** Locked below `invest` by an operator — a gate on an otherwise open product. */
export function isLocked(product: Product): boolean {
  return !isClosed(product) && product.allocation?.caller_access === "view";
}

/** The units stand for an asset held off-platform and the fund holds no cash for them:
 *  `Redeem` is refused (412) and holders exit through the book. A product known only by
 *  a holding reads as cash-backed — the honest default until the detail lands, and the
 *  hub's own default for an unset product. */
export function isInKind(product: Product): boolean {
  return product.allocation?.backing === "in_kind";
}

/** `floor(cash / nav)` in exact base units — mirrors `Shares::from_cash` on the hub, so
 *  the preview cannot disagree with what the ledger actually mints. */
export function unitsForCash(amount: string, nav: string | undefined): bigint | null {
  const cash = toBaseUnits(amount);
  const price = toBaseUnits(nav);
  if (cash <= 0n || price <= 0n) return null;
  return (cash * SCALE) / price;
}

/** `units × nav`, the settle-time estimate shown on redeem. */
export function cashForUnits(units: string, nav: string | undefined): bigint | null {
  const held = toBaseUnits(units);
  const price = toBaseUnits(nav);
  if (held <= 0n || price <= 0n) return null;
  return (held * price) / SCALE;
}

/**
 * Why a fund cannot be subscribed to right now, or `null` when it can.
 *
 * The hub is authoritative on every one of these; stating them here turns a rejection
 * *after* the submit into a sentence *before* it. `remaining_capacity` already nets off
 * in-flight mints server-side, so a client that trusts it can never offer headroom the
 * hub would refuse.
 *
 * Order matters: being locked below `invest` is a gate an operator placed on purpose, so
 * it is checked ahead of the market-condition reasons (a stale mark, a full cap) that
 * would otherwise apply to anyone. `hidden` never reaches here — the detail read is a 404
 * for a caller with no grant, and a granted caller's `caller_access` is their grant — so
 * only `view` distinguishes a locked product from an investable one.
 *
 * Returns the catalogue key rather than the sentence: this module is pure and has no
 * translator, and the reason flows into exactly one render site, which does have one.
 */
export function blockedReasonKey(product: Product, nav: FundNav | null): string | null {
  if (isClosed(product)) return "invest.blocked.closed";
  if (isLocked(product)) return "invest.blocked.locked";
  if (nav?.stale) return "invest.blocked.staleNav";
  if (nav && toBaseUnits(nav.remaining_capacity) <= 0n) return "invest.blocked.capReached";
  return null;
}
