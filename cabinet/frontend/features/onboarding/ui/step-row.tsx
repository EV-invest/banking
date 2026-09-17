"use client";

import { Check, Clock, Lock } from "lucide-react";
import type { ReactNode } from "react";

import { Item, ItemActions, ItemContent, ItemDescription, ItemMedia, ItemTitle } from "@evinvest/uikit";

import type { VerifyState } from "@/features/onboarding/lib/checklist";
import { cn } from "@/shared/lib/cn";

// One step of the path. The state is said twice on purpose — by the mark in the chip and by
// the sentence under the title — because the chip alone is a colour, and a colour is not a
// state to a screen reader or to someone who does not read teal as "done".
//
// `Item` and not three hand-written `div`s: the kit owns this shape (`views/dashboard` uses
// the same composition for operation rows, `features/kyc` for the dialog's steps).

const CHIP: Readonly<Record<VerifyState, string>> = {
  done: "bg-primary/15 text-primary-ink",
  current: "bg-secondary text-ink",
  review: "bg-accent-warn/15 text-accent-warn",
  locked: "bg-secondary text-ink-soft",
};

export function StepRow({ n, state, title, body, action }: { n: number; state: VerifyState; title: string; body: string; action?: ReactNode }) {
  return (
    // A locked step is quieter in colour, never in opacity: `text-ink-soft` at 60 % falls
    // under AA for the description, and what it says — what will open this step — is
    // exactly the sentence a new account is here to read.
    <Item size="sm" className="px-0 py-3 lg:py-4">
      <ItemMedia variant="icon" className={cn("rounded-full border-0", CHIP[state])}>
        {state === "done" ? <Check aria-hidden /> : state === "review" ? <Clock aria-hidden /> : state === "locked" ? <Lock aria-hidden /> : <span className="text-sm font-semibold tabular-nums">{n}</span>}
      </ItemMedia>
      <ItemContent className="min-w-0 gap-0.5">
        <ItemTitle className={cn("block w-auto font-semibold", state === "locked" && "text-ink-soft")}>{title}</ItemTitle>
        <ItemDescription className="line-clamp-none text-xs leading-snug">{body}</ItemDescription>
      </ItemContent>
      {/* i18n-max: 24 — a `shrink-0` control beside `min-w-0` text that wraps. The widest of
          the three CTAs is `dash.browseStrategies` (fr is at 24 today); `kyc.verifyNow` keeps
          its own 14 elsewhere. */}
      {action && <ItemActions className="shrink-0 self-center">{action}</ItemActions>}
    </Item>
  );
}
