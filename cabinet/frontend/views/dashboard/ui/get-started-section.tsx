"use client";

import { useKycStatus, VerifyButton } from "@/features/kyc";
import { type ChecklistShape, ChecklistSkeleton, GetStarted, useChecklist } from "@/features/onboarding";
import { Settled } from "@/shared/ui/motion";

// Where two features meet: verification is `features/kyc`'s knowledge, the path from an
// empty account to a first position is `features/onboarding`'s, and neither may import the
// other — so the view hands one's answer to the other.
//
// It is the first thing on Home, so it holds its place while it reads — in the shape of what
// it is about to become (`ChecklistSkeleton`): a block that arrived after the grid had painted,
// or a placeholder of the wrong height, would move the whole page in front of the reader. It
// stays a browser read even when the money reads came from the server: `/kyc/status` is the
// identity plane's route, which the server has no credential for. A read that failed is the
// one case that leaves nothing — see `useChecklist`.

export function GetStartedSection({ hint, className }: { hint?: ChecklistShape; className?: string }) {
  const state = useChecklist(useKycStatus());
  const { loading, checklist } = state;
  if (!loading && checklist === null) return null;
  return (
    <Settled loading={loading} skeleton={state.loading && <ChecklistSkeleton expect={state.expect} hint={hint} />} className={className}>
      {checklist && <GetStarted checklist={checklist} verifyAction={<VerifyButton size="sm" className="font-semibold" />} />}
    </Settled>
  );
}
