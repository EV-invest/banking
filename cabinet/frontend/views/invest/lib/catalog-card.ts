// What a catalog card says about a product — the CTA it offers and the liquidity terms it
// states — decided once, pure, and away from React so the rules can be run as tests.
//
// The card used to say "Invest" on every product, including one the caller could only
// look at and one that had issued its whole supply. Each of those clicks landed on a page
// whose first sentence was a refusal; the card now says the refusal's way out instead.

import type { FundNav } from "@/shared/contracts";

// Relative and with the extension, like `./product.ts`: the node test runner resolves no
// `@/` alias.
import { toBaseUnits } from "../../../shared/lib/money.ts";
import { isClosed, isInKind, isLocked, type Product } from "./product.ts";

/**
 * The one action a card offers.
 *
 *  · `manage`  — the caller holds units: the page is where they redeem or add.
 *  · `view`    — nothing to do from here (closed, or full with no book); the page explains.
 *  · `locked`  — an operator holds the product below `invest` for this caller.
 *  · `verify`  — the caller is tier 0 and cannot fund a subscription until verified.
 *  · `trade`   — the supply is fully issued and a book is open: the only way in.
 *  · `invest`  — open, and open to this caller.
 */
export type CardCta = "manage" | "view" | "locked" | "verify" | "trade" | "invest";

export interface CardInputs {
  product: Product;
  nav: FundNav | null;
  /** The book policy's `book_open`; `false` while unread, which offers no trade rather than
   *  a trade the terminal would refuse. */
  bookOpen: boolean;
  /** The money gate — tier 0 with the tier actually read (`features/kyc/lib/money-gate`). */
  gated: boolean;
}

/**
 * Order is the same as `blockedReasonKey`: a holding is a fact that outranks every gate; an
 * operator's lock is a gate placed on purpose, so it is named ahead of the caller's own tier;
 * a full cap comes last because it is a market condition rather than a rule about anyone.
 */
export function cardCta({ product, nav, bookOpen, gated }: CardInputs): CardCta {
  if (product.position && toBaseUnits(product.position.units) > 0n) return "manage";
  if (isClosed(product)) return "view";
  if (isLocked(product)) return "locked";
  if (gated) return "verify";
  if (nav && toBaseUnits(nav.remaining_capacity) <= 0n) return bookOpen ? "trade" : "view";
  return "invest";
}

/**
 * How a holder gets their money back.
 *
 * An in-kind product's units stand for an asset the fund holds no cash for, so `Redeem`
 * is refused whatever the book is doing — its exit is the book, open or not yet. A
 * cash-backed product redeems at NAV through the queue, and a book, where one is open,
 * is the second door rather than a replacement for the first.
 */
export type Liquidity = "navQueued" | "navOrBook" | "book";

export function liquidity(product: Product, bookOpen: boolean | undefined): Liquidity | undefined {
  if (isInKind(product)) return "book";
  if (bookOpen === undefined) return undefined;
  return bookOpen ? "navOrBook" : "navQueued";
}
