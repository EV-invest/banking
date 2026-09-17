"use client";

import { useT } from "@evinvest/i18n/react";
import { ArrowLeftRight } from "lucide-react";
import { Fragment } from "react";

import { Badge, Button, Card, CardAction, CardContent, CardHeader, CardTitle, Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle, Item, ItemActions, ItemContent, ItemDescription, ItemGroup, ItemMedia, ItemSeparator, ItemTitle } from "@evinvest/uikit";

import { cn } from "@/shared/lib/cn";
import { Link } from "@/shared/ui/cabinet-link";
import { StaggerItem } from "@/shared/ui/motion";
import { CARD_PAD, EMPTY_BOX } from "@/views/dashboard/lib/chrome";
import type { Op } from "@/views/dashboard/lib/recent-ops";

// The card is a preview, not the record — `/operations` holds the full timeline. The rows
// arrive already shaped (`lib/recent-ops`); this is only how they are drawn.
export function RecentOperationsCard({ ops, className }: { ops: Op[]; className?: string }) {
  const t = useT();
  return (
    <StaggerItem as={Card} className={cn("gap-3 py-4 lg:gap-4 lg:py-5", className)}>
      <CardHeader className={CARD_PAD}>
        <CardTitle>{t("dash.recentOperations")}</CardTitle>
        <CardAction>
          <Button asChild variant="link" size="sm" className="px-0">
            <Link href="/operations">{t("ui.viewAll")}</Link>
          </Button>
        </CardAction>
      </CardHeader>
      <CardContent className={CARD_PAD}>
        {ops.length === 0 ? (
          <Empty className={EMPTY_BOX}>
            <EmptyHeader>
              <EmptyMedia variant="icon">
                <ArrowLeftRight />
              </EmptyMedia>
              <EmptyTitle>{t("ui.noOperations")}</EmptyTitle>
              <EmptyDescription>{t("dash.noOperationsHint")}</EmptyDescription>
            </EmptyHeader>
            <EmptyContent>
              {/* Same destination as the Move money card's filled Deposit, which is
                  already on screen — so this one stays outline. */}
              <Button asChild variant="outline">
                <Link href="/wallet/deposit">{t("ui.addFunds")}</Link>
              </Button>
            </EmptyContent>
          </Empty>
        ) : (
          <ItemGroup>
            {ops.map((op, i) => (
              <Fragment key={op.id}>
                {i > 0 && <ItemSeparator />}
                <Item size="sm" className="px-0 py-3 lg:py-4">
                  <ItemMedia>
                    {/* Decorative: the row title names the kind in words right beside it. */}
                    <Badge className={cn("font-semibold", op.tagClass)}>{op.icon ? <op.icon aria-hidden /> : op.tag}</Badge>
                  </ItemMedia>
                  <ItemContent className="min-w-0 gap-0.5">
                    <ItemTitle className="block w-auto truncate font-semibold">{op.title}</ItemTitle>
                    <ItemDescription className="line-clamp-1 text-xs">{op.sub}</ItemDescription>
                  </ItemContent>
                  <ItemActions className={cn("shrink-0 text-sm font-semibold tabular-nums", op.amountClass)}>{op.amount}</ItemActions>
                </Item>
              </Fragment>
            ))}
          </ItemGroup>
        )}
      </CardContent>
    </StaggerItem>
  );
}
