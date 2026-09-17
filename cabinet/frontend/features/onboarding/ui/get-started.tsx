"use client";

import { useT } from "@evinvest/i18n/react";
import { Link } from "@/shared/ui/cabinet-link";
import { type ReactNode, useEffect, useSyncExternalStore } from "react";

import { Button, Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle, ItemGroup, ItemSeparator } from "@evinvest/uikit";

import { type Checklist, STEP_COUNT, type StepState, type VerifyState } from "@/features/onboarding/lib/checklist";
import { markOpen, stageServerSnapshot, stageSnapshot, subscribeStage } from "@/features/onboarding/lib/checklist-memory";
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
// completion has been seen — see ../lib/checklist-memory.
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

  if (checklist.complete) return stage === "open" ? <AllSet className={className} /> : <CompleteLine className={className} />;

  return (
    <Card className={cn("gap-3 py-4 lg:gap-4 lg:py-5", className)}>
      <CardHeader className={CARD_PAD}>
        <CardTitle>{t("onboarding.title")}</CardTitle>
        <CardDescription>{t("onboarding.subtitle")}</CardDescription>
        <CardAction className="text-xs font-medium tabular-nums text-ink-soft">{t("onboarding.progress", { done: checklist.done, total: STEP_COUNT })}</CardAction>
      </CardHeader>
      <CardContent className={CARD_PAD}>
        <ItemGroup>
          <StepRow n={1} state={checklist.verify} title={t("onboarding.verify.title")} body={t(VERIFY_BODY[checklist.verify])} action={verifyStepAction(checklist.verify, verifyAction, t("profile.kyc.casePill"))} />
          <ItemSeparator />
          <StepRow
            n={2}
            state={checklist.deposit}
            title={t("onboarding.deposit.title")}
            body={t(DEPOSIT_BODY[checklist.deposit])}
            action={
              checklist.deposit === "current" && (
                <Button asChild size="sm">
                  <Link href="/wallet/deposit">{t("ui.deposit")}</Link>
                </Button>
              )
            }
          />
          <ItemSeparator />
          <StepRow
            n={3}
            state={checklist.invest}
            title={t("onboarding.invest.title")}
            body={t(INVEST_BODY[checklist.invest])}
            action={
              checklist.invest === "current" && (
                <Button asChild size="sm">
                  <Link href="/invest">{t("dash.browseStrategies")}</Link>
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
const VERIFY_BODY: Readonly<Record<VerifyState, string>> = {
  current: "kyc.dialog.timeBody",
  review: "onboarding.verify.review",
  done: "onboarding.verify.done",
  // Unreachable — the first step is never behind another — but the map is total so a state
  // added later cannot fall through to an empty line.
  locked: "kyc.dialog.timeBody",
};

const DEPOSIT_BODY: Readonly<Record<StepState, string>> = {
  locked: "onboarding.deposit.locked",
  current: "onboarding.deposit.current",
  done: "onboarding.deposit.done",
};

const INVEST_BODY: Readonly<Record<StepState, string>> = {
  locked: "onboarding.invest.locked",
  current: "onboarding.invest.current",
  done: "onboarding.invest.done",
};

/** In review there is nothing to press: a pending mark stands where the button would. */
function verifyStepAction(state: VerifyState, verifyAction: ReactNode, reviewLabel: string): ReactNode {
  if (state === "current") return verifyAction;
  if (state === "review") return <Pill tone="pending">{reviewLabel}</Pill>;
  return null;
}
