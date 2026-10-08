"use client";

// The way into the terminal from a product page — offered only while the product's book
// is open. Reads the policy itself so the product page stays ignorant of the book: a
// closed or unconfigured book simply has no control, rather than a disabled one that
// invites the question "why".

import { ChartCandlestick } from "lucide-react";

import { useT } from "@evinvest/i18n/react";
import { Button } from "@evinvest/uikit";

import { bookPolicyResource } from "@/entities/book/model/book-resource";
import type { BookPolicy } from "@/shared/contracts/book";
import { type ResourceSeed, useSeededResource } from "@/shared/lib/resource";
import { Link } from "@/shared/ui/cabinet-link";

// `seed` is the product page's server read of the same policy: without it the server and the
// hydrating render have no policy, and the control would appear only after hydration.
export function TradeLink({ service, seed }: { service: string; seed?: ResourceSeed<BookPolicy> }) {
  const t = useT();
  const policy = useSeededResource(bookPolicyResource, seed, service);
  if (!policy.data?.book_open) return null;
  return (
    <Button asChild variant="outline">
      <Link href={`/invest/${encodeURIComponent(service)}/trade`}>
        <ChartCandlestick className="size-4" />
        {t("invest.trade", "Trade")}
      </Link>
    </Button>
  );
}
