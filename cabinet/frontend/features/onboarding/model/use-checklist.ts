"use client";

// The checklist as Home reads it: the wallet and the positions this slice may read for
// itself (they are the entity reads Home already holds, cached — no request is added), and
// the verification state, which belongs to `features/kyc` and is handed in by the view
// because one feature does not reach into another.

import { positionsResource } from "@/entities/fund/model/fund-resource";
import { profileResource } from "@/entities/user/model/profile-resource";
import { walletResource } from "@/entities/wallet/model/wallet-resource";
import { type Checklist, deriveChecklist } from "@/features/onboarding/lib/checklist";
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

export type ChecklistState =
  /** Something is still in flight — the surface owes a skeleton, not a verdict. */
  | { loading: true; checklist: null }
  /** Every read has finished; `null` means one of them failed and nothing can be said. */
  | { loading: false; checklist: Checklist | null };

/**
 * "Settled" is deliberately not "not loading": each read can finish by failing, and a tier
 * nobody could read is not a tier of 0 — a checklist drawn on that would tell a verified
 * reader to verify over an unrelated blip (the rule `features/kyc/lib/money-gate` states for
 * the wallet). On that the block simply does not appear.
 */
export function useChecklist(kyc: VerificationRead): ChecklistState {
  const { data: profile } = useResource(profileResource);
  const wallet = useResource(walletResource);
  const positions = useResource(positionsResource);

  if (kyc.loading || wallet.isLoading || positions.isLoading) return { loading: true, checklist: null };

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
