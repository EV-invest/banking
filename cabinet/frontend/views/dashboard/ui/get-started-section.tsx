"use client";

import { Skeleton } from "@evinvest/uikit";

import { useKycStatus, VerifyButton } from "@/features/kyc";
import { GetStarted, useChecklist } from "@/features/onboarding";
import { Settled } from "@/shared/ui/motion";

// Where two features meet: verification is `features/kyc`'s knowledge, the path from an
// empty account to a first position is `features/onboarding`'s, and neither may import the
// other — so the view hands one's answer to the other.
//
// It is the first thing on Home, so it holds its place while it reads: a block that arrived
// after the grid had painted would push the whole page down in front of the reader. A read
// that failed is the one case that leaves nothing — see `useChecklist`.

export function GetStartedSection({ className }: { className?: string }) {
  const { loading, checklist } = useChecklist(useKycStatus());
  if (!loading && checklist === null) return null;
  return (
    <Settled loading={loading} skeleton={<Skeleton className="h-24 w-full" />} className={className}>
      {checklist && <GetStarted checklist={checklist} verifyAction={<VerifyButton size="sm" className="font-semibold" />} />}
    </Settled>
  );
}
