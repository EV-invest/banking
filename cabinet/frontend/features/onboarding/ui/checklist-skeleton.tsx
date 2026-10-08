"use client";

import { useT } from "@evinvest/i18n/react";
import { type ReactNode, useSyncExternalStore } from "react";

import { Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle, Item, ItemActions, ItemContent, ItemDescription, ItemGroup, ItemMedia, ItemSeparator, ItemTitle } from "@evinvest/uikit";

import { completionView, stageServerSnapshot, stageSnapshot, subscribeStage } from "@/features/onboarding/lib/checklist-memory";
import type { ChecklistShape } from "@/features/onboarding/lib/checklist-shape";
import type { ChecklistExpect } from "@/features/onboarding/model/use-checklist";
import { cn } from "@/shared/lib/cn";
import { useHydrated } from "@/shared/lib/use-hydrated";

// The block while its reads are out, in the shape of what it is about to become.
//
// Built from the very components and copy the finished block uses, with the words hidden
// behind skeleton bars rather than replaced by fixed-height ones: the copy wraps differently
// in every locale and at every width, and only the copy itself takes exactly its height. No
// state words are rendered visibly — the bars are the only thing a reader sees until every
// read has answered, so no frame can claim a step is done — and a line that would be a
// verdict ("0 of 3 done", "You're all set", the status line) is a plain bar, not hidden copy.

const CARD_PAD = "px-4 lg:px-6";

/** Copy laid out as the real block lays it out, drawn as a skeleton. Hidden from assistive tech. */
function Ghost({ children }: { children: ReactNode }) {
  return (
    <span data-slot="skeleton" className="rounded-md bg-hover box-decoration-clone text-transparent select-none">
      {children}
    </span>
  );
}

/**
 * `hint` is the shape this browser last settled on, as the server read it from the cookie.
 * The server and the hydrating render go by it — neither can see the stage in
 * `localStorage`. With no hint they render every shape the stage could pick and let CSS keep
 * the one `StageMirror` chose before the first paint (`globals.css`, "Onboarding stage").
 * Once hydrated, what the reads and the stage say wins; with neither the money reads nor a
 * stage that says the path was finished here, it is the card.
 */
export function ChecklistSkeleton({ expect, hint, className }: { expect: ChecklistExpect; hint?: ChecklistShape; className?: string }) {
  const stage = useSyncExternalStore(subscribeStage, stageSnapshot, stageServerSnapshot);
  const hydrated = useHydrated();
  if (!hydrated && hint === undefined && expect !== "path") return <StageCandidates expect={expect} className={className} />;
  const known: ChecklistShape | null = expect === "unknown" ? null : expect === "path" ? "path" : (completionView(stage) ?? "none");
  const guess: ChecklistShape = stage === "acknowledged" ? "line" : "path";
  const shape = hydrated ? (known ?? hint ?? guess) : (hint ?? known ?? "path");
  if (shape === "path") return <PathSkeleton className={className} />;
  if (shape === "all-set") return <AllSetSkeleton className={className} />;
  if (shape === "line") return <LineSkeleton className={className} />;
  return null;
}

// The same elements the single shape renders, one class each — no wrapper, so the one CSS
// keeps lays out exactly as it will once hydration swaps the set for it. "Unknown" is a path
// still to walk unless this browser finished it; "complete" collapses by the stage.
function StageCandidates({ expect, className }: { expect: "unknown" | "complete"; className?: string }) {
  if (expect === "unknown") {
    return (
      <>
        <PathSkeleton className={cn(className, "ev-stage-unless-acknowledged")} />
        <LineSkeleton className={cn(className, "ev-stage-if-acknowledged")} />
      </>
    );
  }
  return (
    <>
      <AllSetSkeleton className={cn(className, "ev-stage-if-open")} />
      <LineSkeleton className={cn(className, "ev-stage-if-acknowledged")} />
    </>
  );
}

function PathSkeleton({ className }: { className?: string }) {
  const t = useT();
  // A new account's card: the first step carries the action, the other two are locked.
  const rows = [
    { title: t("onboarding.verify.title", "Verify your identity"), body: t("kyc.dialog.timeBody", "A few minutes to submit. Most checks are decided within the hour."), action: true },
    { title: t("onboarding.deposit.title", "Make your first deposit"), body: t("onboarding.deposit.locked", "Opens once your identity is verified."), action: false },
    { title: t("onboarding.invest.title", "Choose a strategy"), body: t("onboarding.invest.locked", "Opens once a deposit is credited."), action: false },
  ];
  return (
    <Card aria-hidden className={cn("gap-3 py-4 lg:gap-4 lg:py-5", className)}>
      <CardHeader className={CARD_PAD}>
        <CardTitle>
          <Ghost>{t("onboarding.title", "Get started")}</Ghost>
        </CardTitle>
        <CardDescription>
          <Ghost>{t("onboarding.subtitle", "Three steps to your first investment.")}</Ghost>
        </CardDescription>
        <CardAction className="text-xs font-medium">
          <Bar className="h-4 w-16" />
        </CardAction>
      </CardHeader>
      <CardContent className={CARD_PAD}>
        <ItemGroup>
          {rows.map((row, i) => (
            <PathRow key={row.title} first={i === 0} {...row} />
          ))}
        </ItemGroup>
      </CardContent>
    </Card>
  );
}

function PathRow({ first, title, body, action }: { first: boolean; title: string; body: string; action: boolean }) {
  return (
    <>
      {!first && <ItemSeparator />}
      {/* The same `Item` as `StepRow`, so the row takes the height the step will. */}
      <Item size="sm" className="px-0 py-3 lg:py-4">
        <ItemMedia variant="icon" data-slot="skeleton" className="rounded-full border-0 bg-hover" />
        <ItemContent className="min-w-0 gap-0.5">
          <ItemTitle className="block w-auto font-semibold">
            <Ghost>{title}</Ghost>
          </ItemTitle>
          <ItemDescription className="line-clamp-none text-xs leading-snug">
            <Ghost>{body}</Ghost>
          </ItemDescription>
        </ItemContent>
        {action && (
          <ItemActions className="shrink-0 self-center">
            <Bar className="h-8 w-24" />
          </ItemActions>
        )}
      </Item>
    </>
  );
}

function AllSetSkeleton({ className }: { className?: string }) {
  const t = useT();
  return (
    <Card aria-hidden className={cn("py-4 lg:py-5", className)}>
      <Item size="sm" className="px-4 py-0 lg:px-6">
        <ItemMedia variant="icon" data-slot="skeleton" className="rounded-lg border-0 bg-hover" />
        <ItemContent className="min-w-0 gap-0.5">
          <ItemTitle className="block w-auto font-semibold">
            <Bar className="h-5 w-28" />
          </ItemTitle>
          <ItemDescription className="line-clamp-none leading-snug">
            <Ghost>{t("onboarding.allSet.body", "Your holdings and their split are under {own}; every movement lands in {ops}.", { own: t("dash.investedWhatIOwn", "Invested · what I own"), ops: t("dash.recentOperations", "Recent activity") })}</Ghost>
          </ItemDescription>
        </ItemContent>
        <ItemActions className="shrink-0 self-center">
          <Bar className="h-8 w-16" />
        </ItemActions>
      </Item>
    </Card>
  );
}

function LineSkeleton({ className }: { className?: string }) {
  return (
    <p aria-hidden className={cn("flex items-center gap-2 text-xs font-medium", className)}>
      <Bar className="size-4 rounded-full" />
      {/* `text-xs` sets a 16px line, the height of the status line's icon and text alike. */}
      <Bar className="h-4 w-44" />
    </p>
  );
}

function Bar({ className }: { className?: string }) {
  return <span data-slot="skeleton" className={cn("block rounded-md bg-hover", className)} />;
}
