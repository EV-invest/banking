"use client";

// Starting verification, as a state machine the presentations share.
//
// There are two of them — a row inside the profile's Verification card, and a block standing
// where a money surface would be for a caller the hub has not cleared yet — and they differ
// only in their frame. What must not differ is any of this: that a start in flight disables
// its own control, that `started` leaves the page instead of returning to idle, and which
// outcomes are worth a sentence of their own.

import { useState } from "react";

import { startVerification, type KycStart } from "@/features/kyc/api/kyc-client";

/** Idle, in flight, or what came back — every outcome but `started`, which leaves this page. */
export type StartState = { kind: "idle" | "starting" } | Exclude<KycStart, { kind: "started" }>;

export interface StartVerification {
  state: StartState;
  starting: boolean;
  begin: () => Promise<void>;
}

export function useStartVerification(): StartVerification {
  const [state, setState] = useState<StartState>({ kind: "idle" });
  const starting = state.kind === "starting";

  async function begin() {
    if (starting) return;
    setState({ kind: "starting" });
    const result = await startVerification();
    if (result.kind === "started") {
      // Deliberately NOT back to idle first: the provider's page is already loading over
      // this one, and re-enabling the control would flash a second chance at someone who
      // is leaving — and buy a duplicate case if they took it.
      window.location.assign(result.redirectUrl);
      return;
    }
    setState(result);
  }

  return { state, starting, begin };
}
