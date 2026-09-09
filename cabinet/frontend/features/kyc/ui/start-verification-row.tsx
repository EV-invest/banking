"use client";

import { Loader2 } from "lucide-react";
import { useState } from "react";

import { Button } from "@evinvest/uikit";

import { startVerification, type KycStart } from "@/features/kyc/api/kyc-client";
import { isVerificationPending, markVerificationPending } from "@/features/kyc/lib/pending-case";
import { Hairline, Pill, RowLabel } from "@/shared/ui/list-card";

// The one place in the cabinet a user can begin identity verification themselves. It sits
// inside `card-Verification`, below the rows that state what the hub currently holds, so
// the card reads as status-then-action rather than as a form.

type State = { kind: "idle" | "starting" | "pending" } | Exclude<KycStart, { kind: "started" }>;

// `email` identifies whose pending flag this is (see `pending-case.ts`) — the caller only
// ever renders this row for the signed-in user's own profile.
export function StartVerificationRow({ email }: { email: string }) {
  const [state, setState] = useState<State>(() => (isVerificationPending(email) ? { kind: "pending" } : { kind: "idle" }));
  const starting = state.kind === "starting";

  async function begin() {
    setState({ kind: "starting" });
    const result = await startVerification();
    if (result.kind === "started") {
      // Set before the redirect, not after: a vendor session costs money, so the flag has
      // to be in place before the user can possibly land back here and press Start again.
      markVerificationPending(email);
      // Deliberately NOT back to idle first: the provider's page is already loading over
      // this one, and re-enabling the button would flash a second chance at someone who
      // is leaving — and buy a duplicate case if they took it.
      window.location.assign(result.redirectUrl);
      return;
    }
    setState(result);
  }

  if (state.kind === "pending") {
    return (
      <>
        <Hairline />
        <div className="flex min-w-0 items-center justify-between gap-3 py-3.5">
          <RowLabel title="Verify your identity" sub="Submitted — we're waiting on the provider's review" />
          <Pill tone="pending">Pending</Pill>
        </div>
      </>
    );
  }

  return (
    <>
      <Hairline />
      <div className="flex flex-col py-3.5">
        <div className="flex min-w-0 items-center justify-between gap-3">
          <RowLabel title="Verify your identity" sub="A photo of your ID and a selfie — a few minutes" />
          <Button type="button" size="sm" onClick={begin} disabled={starting} aria-busy={starting} className="rounded-lg font-semibold">
            {starting && <Loader2 className="size-4 animate-spin" aria-hidden />}
            Start
          </Button>
        </div>
        {/* The live region is mounted empty and stays mounted: a `role="status"` inserted
            in the same tick as its own text is unreliably announced. It carries no gap of
            its own so an empty one takes no space — the message brings its own margin. */}
        <div role="status">
          <Outcome state={state} />
        </div>
      </div>
    </>
  );
}

function Outcome({ state }: { state: State }) {
  // The vendor's actual trouble — no balance, no configuration, an outage — is ours to fix
  // and never reaches the browser, so this says only that it cannot run and offers the one
  // address that can help.
  if (state.kind === "unavailable") {
    return (
      <p className="mt-2 text-xs leading-snug text-muted-foreground">
        Verification isn&apos;t available right now. Please try again later
        {state.contact ? (
          <>
            {" or contact "}
            <a
              href={`mailto:${state.contact}`}
              className="font-medium text-main-accent-t1 underline underline-offset-2 outline-none focus-visible:ring-2 focus-visible:ring-ring"
            >
              {state.contact}
            </a>
          </>
        ) : null}
        .
      </p>
    );
  }
  if (state.kind === "failed") {
    return (
      <p className="mt-2 text-xs leading-snug text-destructive">
        {state.message}
      </p>
    );
  }
  return null;
}
