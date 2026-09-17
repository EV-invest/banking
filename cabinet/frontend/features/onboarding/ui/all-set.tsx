"use client";

import { useT } from "@evinvest/i18n/react";

import { CircleCheck, Sparkles } from "lucide-react";

import { Button, Card, Item, ItemActions, ItemContent, ItemDescription, ItemMedia, ItemTitle } from "@evinvest/uikit";

import { acknowledge } from "@/features/onboarding/lib/checklist-memory";
import { cn } from "@/shared/lib/cn";

// What the checklist becomes once the last step is done.
//
// First, once, a card that says where to look now — the two Home cards that were empty
// frames a minute ago. It names them by their own titles (interpolated, so a rename there
// is a rename here) rather than linking, because both are on this very page. The one
// button puts it away; this is the only point in the block's life where a dismissal exists.
//
// The same `Item` composition as a step row, so the finish line reads as the path's last
// row rather than as a notice from somewhere else.

export function AllSet({ className }: { className?: string }) {
  const t = useT();
  return (
    <Card className={cn("py-4 lg:py-5", className)}>
      <Item size="sm" className="px-4 py-0 lg:px-6">
        <ItemMedia variant="icon" className="rounded-lg border-0 bg-primary/15 text-primary-ink">
          <Sparkles aria-hidden />
        </ItemMedia>
        <ItemContent className="min-w-0 gap-0.5">
          <ItemTitle className="block w-auto font-semibold">{t("onboarding.allSet.title")}</ItemTitle>
          <ItemDescription className="line-clamp-none leading-snug">{t("onboarding.allSet.body", { own: t("dash.investedWhatIOwn"), ops: t("dash.recentOperations") })}</ItemDescription>
        </ItemContent>
        <ItemActions className="shrink-0 self-center">
          <Button type="button" variant="outline" size="sm" onClick={acknowledge}>
            {t("onboarding.allSet.dismiss")}
          </Button>
        </ItemActions>
      </Item>
    </Card>
  );
}

/** Then, for good: the collapsed one-line status the path leaves behind. */
export function CompleteLine({ className }: { className?: string }) {
  const t = useT();
  return (
    <p className={cn("flex items-center gap-2 text-xs font-medium text-ink-soft", className)}>
      <CircleCheck className="size-4 text-primary-ink" aria-hidden />
      {t("onboarding.status.complete")}
    </p>
  );
}
