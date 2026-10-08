"use client";

// The checklist as Home reads it: the wallet and the positions this slice may read for
// itself (they are the entity reads Home already holds, cached — no request is added), and
// the verification state, which belongs to `features/kyc` and is handed in by the view
// because one feature does not reach into another.

import { useEffect, useSyncExternalStore } from "react";

import { positionsResource } from "@/entities/fund/model/fund-resource";
import { profileResource } from "@/entities/user/model/profile-resource";
import { walletResource } from "@/entities/wallet/model/wallet-resource";
import { type Checklist, deriveChecklist } from "@/features/onboarding/lib/checklist";
import { completionView, stageServerSnapshot, stageSnapshot, subscribeStage } from "@/features/onboarding/lib/checklist-memory";
import { type ChecklistShape, shapeCookie } from "@/features/onboarding/lib/checklist-shape";
import type { PositionList, Wallet } from "@/shared/contracts";
import { num } from "@/shared/lib/money";
import { useResource } from "@/shared/lib/resource";

/** The verification signals the checklist needs — structurally, `features/kyc`'s `KycGate`. */
export interface VerificationRead {
  level: number;
  runningCase: { resumable: boolean } | null;
  /** `false` means the identity plane did not answer and `level` came from the profile. */
  known: boolean;
  loading: boolean;
}

/**
 * What the block is about to become, for the skeleton to take the shape of while the reads
 * finish: `path` is the three-step card, `complete` is whatever a finished path collapses to,
 * `unknown` is "the money reads have not answered either" — the skeleton then trusts the
 * shape this browser last saw (`lib/checklist-shape`). Without it a veteran's skeleton would
 * be a card that then folds to nothing, and a new account's a sliver that then pushes the
 * page down.
 */
export type ChecklistExpect = "path" | "complete" | "unknown";

export type ChecklistState =
  /** Something is still in flight — the surface owes a skeleton, not a verdict. */
  | { loading: true; checklist: null; expect: ChecklistExpect }
  /** Every read has finished; `null` means one of them failed and nothing can be said. */
  | { loading: false; checklist: Checklist | null };

/**
 * "Settled" is deliberately not "not loading": each read can finish by failing, and a tier
 * nobody could read is not a tier of 0 — a checklist drawn on that would tell a verified
 * reader to verify over an unrelated blip (the rule `features/kyc/lib/money-gate` states for
 * the wallet). On that the block simply does not appear.
 */
export function useChecklist(kyc: VerificationRead): ChecklistState {
  const state = useChecklistState(kyc);
  const stage = useSyncExternalStore(subscribeStage, stageSnapshot, stageServerSnapshot);
  // A settled block remembers its shape for the next server render (see `lib/checklist-shape`).
  // A failed read says nothing about the account, so it leaves the last shape standing.
  const shape: ChecklistShape | null = state.loading || state.checklist === null ? null : !state.checklist.complete ? "path" : (completionView(stage) ?? "none");
  useEffect(() => {
    if (shape !== null) document.cookie = shapeCookie(shape);
  }, [shape]);
  return state;
}

function useChecklistState(kyc: VerificationRead): ChecklistState {
  const { data: profile } = useResource(profileResource);
  const wallet = useResource(walletResource);
  const positions = useResource(positionsResource);

  if (kyc.loading || wallet.isLoading || positions.isLoading) return { loading: true, checklist: null, expect: expectFrom(wallet.data, positions.data) };

  const settled = kyc.known || profile != null;
  if (!settled || wallet.data === undefined || positions.data === undefined) return { loading: false, checklist: null };

  return {
    loading: false,
    checklist: deriveChecklist({
      level: kyc.level,
      runningCase: kyc.runningCase,
      total: num(wallet.data.balance?.total),
      positions: (positions.data.positions ?? []).length,
    }),
  };
}

/**
 * Funded and invested settle two of the three steps on the money reads alone, and an account
 * that got that far has, in practice, passed verification (the hub refuses a deposit address
 * below the entry tier) — so it is shaped as finished while `/kyc/status` is still out. Only
 * the shape: the block's words wait for every read, so no frame states a step as done that
 * the reads have not.
 */
function expectFrom(wallet: Wallet | undefined, positions: PositionList | undefined): ChecklistExpect {
  if (wallet === undefined || positions === undefined) return "unknown";
  return num(wallet?.balance?.total) > 0 && (positions?.positions ?? []).length > 0 ? "complete" : "path";
}
