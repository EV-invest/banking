"use client";

// The way into the terminal from a product page — offered only while the product's book
// is open. Reads the policy itself so the product page stays ignorant of the book: a
// closed or unconfigured book simply has no control, rather than a disabled one that
// invites the question "why".

import { ChartCandlestick } from "lucide-react";

import { useT } from "@evinvest/i18n/react";
import { Button } from "@evinvest/uikit";

import { bookPolicyResource } from "@/entities/book/model/book-resource";
import { useResource } from "@/shared/lib/resource";
import { Link } from "@/shared/ui/cabinet-link";

export function TradeLink({ service }: { service: string }) {
  const t = useT();
  const policy = useResource(bookPolicyResource, service);
  if (!policy.data?.book_open) return null;
  return (
    <Button asChild variant="outline">
      <Link href={`/invest/${encodeURIComponent(service)}/trade`}>
        <ChartCandlestick className="size-4" />
        {t("invest.trade")}
      </Link>
    </Button>
  );
}
