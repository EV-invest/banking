"use client";

// The strip across the top: which product, what it last traded at, how the day went, and
// — beside the book's price — the fund's own NAV. The two are different facts and are
// labelled as such: the book is what holders will pay each other, the NAV is what the
// operator marked the fund at. A terminal that showed one number would be hiding the gap
// that is the whole reason a secondary market exists.

import { ArrowLeft } from "lucide-react";

import { useT } from "@evinvest/i18n/react";
import { Badge, TerminalTicker, TickerStat } from "@evinvest/uikit";

import { bookSnapshotResource } from "@/entities/book/model/book-resource";
import type { BookStreamStatus } from "@/entities/book/model/book-socket";
import { cn } from "@/shared/lib/cn";
import { useResource } from "@/shared/lib/resource";
import { Link } from "@/shared/ui/cabinet-link";
import { ProductIcon, productTone } from "@/shared/ui/icons/products";
import type { Product } from "@/views/invest/lib/product";
import { formatChange, formatUnits, formatUsdt, isFall } from "@/views/trade/lib/format";

export function TickerPane({ product, status }: { product: Product; status: BookStreamStatus }) {
  const t = useT();
  const book = useResource(bookSnapshotResource, product.service).data ?? null;
  const price = (value: string | undefined) => (value ? formatUsdt(value) : "—");
  const fall = isFall(book?.change_24h);

  return (
    <TerminalTicker>
      {/* The way back is the product page, not the catalog: the terminal is a detail of
          the product, and the same mark and tint identify which one. */}
      <Link
        href={`/invest/${encodeURIComponent(product.service)}`}
        className="flex shrink-0 items-center gap-2.5 rounded-md pr-2 outline-none transition-colors hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
      >
        <ArrowLeft className="size-4 text-muted-foreground" />
        <span className={cn("flex size-7 shrink-0 items-center justify-center rounded-lg", productTone(product.service))}>
          <ProductIcon icon={product.icon} className="size-4" />
        </span>
        <span className="min-w-0">
          <span className="block truncate text-sm font-semibold">{product.title}</span>
          <span className="block font-mono-tech text-xs text-muted-foreground">{product.service}</span>
        </span>
      </Link>
      <TickerStat label={t("trade.ticker.last")} value={price(book?.last_price)} className={book?.last_side === "sell" ? "text-accent-error" : book?.last_side === "buy" ? "text-positive" : undefined} />
      <TickerStat label={t("trade.ticker.change")} value={formatChange(book?.change_24h)} className={book?.change_24h ? (fall ? "text-accent-error" : "text-positive") : undefined} />
      <TickerStat label={t("trade.ticker.volume")} value={book?.volume_24h ? formatUnits(book.volume_24h) : "—"} />
      <TickerStat label={t("trade.ticker.mid")} value={price(book?.mid)} />
      <TickerStat label={t("trade.ticker.spread")} value={price(book?.spread)} />
      <TickerStat label={t("trade.ticker.nav")} value={price(book?.nav)} title={t("trade.ticker.navHint")} />
      <StreamChip status={status} />
    </TerminalTicker>
  );
}

/** How the book is being kept current — quiet, and honest about the difference. The page
 *  stays correct either way (the poll runs under a down socket); only latency is lost. */
function StreamChip({ status }: { status: BookStreamStatus }) {
  const t = useT();
  if (status === "idle") return null;
  const live = status === "live";
  return (
    <Badge variant="outline" className="ml-auto shrink-0 gap-1.5 rounded-full font-medium text-muted-foreground">
      <span className={cn("size-1.5 rounded-full", live ? "bg-positive" : status === "paused" ? "bg-muted-foreground" : "animate-pulse bg-accent-warn")} />
      {t(`trade.stream.${status}`)}
    </Badge>
  );
}
