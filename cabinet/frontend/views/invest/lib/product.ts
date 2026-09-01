// The invest surface's shared vocabulary: what a "product" is on screen, and the exact
// arithmetic the two deal panels preview with.
//
// Everything here is pure and display-only. The hub re-derives every figure server-side;
// these exist so a holder sees what they are about to get *before* submitting, in the
// same rounding the ledger will actually apply.

import type { Allocation, FundNav, Position } from "@/shared/contracts";

import { toBaseUnits } from "./format";

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
  for (const a of catalog) {
    byService.set(a.service, { service: a.service, title: a.title, summary: a.summary, allocation: a, position: null });
  }
  for (const p of positions) {
    const service = p.service ?? "";
    if (!service) continue;
    const existing = byService.get(service);
    if (existing) existing.position = p;
    else byService.set(service, { service, title: service, summary: "", allocation: null, position: p });
  }
  return [...byService.values()].sort((a, b) => a.title.localeCompare(b.title));
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
 * Returns the catalogue key rather than the sentence: this module is pure and has no
 * translator, and the reason flows into exactly one render site, which does have one.
 */
export function blockedReasonKey(product: Product, nav: FundNav | null): string | null {
  if (product.allocation === null) return "invest.blocked.closed";
  if (nav?.stale) return "invest.blocked.staleNav";
  if (nav && toBaseUnits(nav.remaining_capacity) <= 0n) return "invest.blocked.capReached";
  return null;
}
