"use client";

import type { Translate } from "@evinvest/i18n";
import { useT } from "@evinvest/i18n/react";
import { Link } from "@/shared/ui/cabinet-link";
import { type ReactNode, useEffect, useSyncExternalStore } from "react";

import { Button, Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle, ItemGroup, ItemSeparator } from "@evinvest/uikit";

import { type Checklist, STEP_COUNT, type StepState, type VerifyState } from "@/features/onboarding/lib/checklist";
import { completionView, markOpen, stageServerSnapshot, stageSnapshot, subscribeStage } from "@/features/onboarding/lib/checklist-memory";
import { AllSet, CompleteLine } from "@/features/onboarding/ui/all-set";
import { StepRow } from "@/features/onboarding/ui/step-row";
import { cn } from "@/shared/lib/cn";
import { Pill } from "@/shared/ui/list-card";

// The first thing a new account sees on Home: the path from an empty account to a first
// position, as three steps with one action each (#382). It replaces the dismissible
// verification banner, which explained the first step and nothing after it — and once put
// away left a new investor with a zero balance, an empty plot and a filled Deposit button
// that led to a closed door.
//
// Not dismissible while a step is open. What is remembered per browser is only whether the
// completion has been seen — see ../lib/checklist-memory, which also decides that a browser
// that never saw a step open shows nothing for a finished path.
//
// The verify step's control is the caller's to hand in: starting verification belongs to
// `features/kyc`, and one feature does not import another. Every state other than "current"
// is decided here, so the slot is only ever rendered when a start may actually be offered.

const CARD_PAD = "px-4 lg:px-6";

export function GetStarted({ checklist, verifyAction, className }: { checklist: Checklist; verifyAction: ReactNode; className?: string }) {
  const t = useT();
  const stage = useSyncExternalStore(subscribeStage, stageSnapshot, stageServerSnapshot);

  // A step on screen is the "before" a later completion needs — recorded as a side effect
  // of rendering it, so nothing has to be clicked for a finish to count.
  useEffect(() => {
    if (!checklist.complete) markOpen();
  }, [checklist.complete]);

  if (checklist.complete) {
    const view = completionView(stage);
    if (view === "all-set") return <AllSet className={className} />;
    if (view === "line") return <CompleteLine className={className} />;
    return null;
  }

  return (
    <Card className={cn("gap-3 py-4 lg:gap-4 lg:py-5", className)}>
      <CardHeader className={CARD_PAD}>
        <CardTitle>{t("onboarding.title", "Get started")}</CardTitle>
        <CardDescription>{t("onboarding.subtitle", "Three steps to your first investment.")}</CardDescription>
        <CardAction className="text-xs font-medium tabular-nums text-ink-soft">{t("onboarding.progress", "{done} of {total} done", { done: checklist.done, total: STEP_COUNT })}</CardAction>
      </CardHeader>
      <CardContent className={CARD_PAD}>
        <ItemGroup>
          <StepRow n={1} state={checklist.verify} title={t("onboarding.verify.title", "Verify your identity")} body={verifyBody(t)[checklist.verify]} action={verifyStepAction(checklist.verify, verifyAction, t("profile.kyc.casePill", "In review"))} />
          <ItemSeparator />
          <StepRow
            n={2}
            state={checklist.deposit}
            title={t("onboarding.deposit.title", "Make your first deposit")}
            body={depositBody(t)[checklist.deposit]}
            action={
              checklist.deposit === "current" && (
                <Button asChild size="sm">
                  <Link href="/wallet/deposit">{t("ui.deposit", "Deposit")}</Link>
                </Button>
              )
            }
          />
          <ItemSeparator />
          <StepRow
            n={3}
            state={checklist.invest}
            title={t("onboarding.invest.title", "Choose a strategy")}
            body={investBody(t)[checklist.invest]}
            action={
              checklist.invest === "current" && (
                <Button asChild size="sm">
                  <Link href="/invest">{t("dash.browseStrategies", "Browse strategies")}</Link>
                </Button>
              )
            }
          />
        </ItemGroup>
      </CardContent>
    </Card>
  );
}

// The time expectations are the owner-approved wording, reused: `kyc.dialog.timeBody` for
// the check, "credited once the network confirms it" for the deposit. No figure appears
// that the dialog does not already state.
// TODO(#385): sourced figures
const verifyBody = (t: Translate): Readonly<Record<VerifyState, string>> => ({
  current: t("kyc.dialog.timeBody", "A few minutes to submit. Most checks are decided within the hour."),
  review: t("onboarding.verify.review", "Nothing to do on your side — your documents are with our reviewers and we'll email you the decision."),
  done: t("onboarding.verify.done", "Your deposit address and withdrawals are open."),
  // Unreachable — the first step is never behind another — but the map is total so a state
  // added later cannot fall through to an empty line.
  locked: t("kyc.dialog.timeBody", "A few minutes to submit. Most checks are decided within the hour."),
});

const depositBody = (t: Translate): Readonly<Record<StepState, string>> => ({
  locked: t("onboarding.deposit.locked", "Opens once your identity is verified."),
  current: t("onboarding.deposit.current", "Send funds to your deposit address — credited once the network confirms it."),
  done: t("onboarding.deposit.done", "Funds received."),
});

const investBody = (t: Translate): Readonly<Record<StepState, string>> => ({
  locked: t("onboarding.invest.locked", "Opens once a deposit is credited."),
  current: t("onboarding.invest.current", "Pick a strategy and subscribe with your available balance."),
  done: t("onboarding.invest.done", "You hold units."),
});

/** In review there is nothing to press: a pending mark stands where the button would. */
function verifyStepAction(state: VerifyState, verifyAction: ReactNode, reviewLabel: string): ReactNode {
  if (state === "current") return verifyAction;
  if (state === "review") return <Pill tone="pending">{reviewLabel}</Pill>;
  return null;
}
