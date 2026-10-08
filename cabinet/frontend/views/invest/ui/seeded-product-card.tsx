"use client";

// The `/invest` card as the route streams it: the lists arrive first, each card's own reads
// a step later (see the route), so the card waits for its seed under a boundary of its own.

import { Suspense, use } from "react";

import { type ProductCardSeed, ProductCard, ProductCardBody } from "@/views/invest/ui/product-card";
import type { Product } from "@/views/invest/lib/product";

/**
 * A card whose reads the server is still making: until they land it is the card in its
 * loading state, so the list above it is not held back by them. That placeholder reads
 * nothing — the browser can mount it before the server's answers arrive, and a card that
 * subscribed there would ask the BFF the very questions already on their way.
 */
export function SeededProductCard({ seeds, product, gated }: { seeds?: Promise<Readonly<Record<string, ProductCardSeed>>>; product: Product; gated: boolean }) {
  if (!seeds) return <ProductCard product={product} gated={gated} />;
  return (
    <Suspense fallback={<ProductCardBody product={product} gated={gated} nav={null} navFailed={false} policy={undefined} bookOpen={undefined} />}>
      <CardFromSeeds seeds={seeds} product={product} gated={gated} />
    </Suspense>
  );
}

function CardFromSeeds({ seeds, product, gated }: { seeds: Promise<Readonly<Record<string, ProductCardSeed>>>; product: Product; gated: boolean }) {
  return <ProductCard product={product} gated={gated} seed={use(seeds)[product.service]} />;
}
