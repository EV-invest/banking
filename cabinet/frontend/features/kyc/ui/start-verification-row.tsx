"use client";

import { useT } from "@evinvest/i18n/react";

import { useStartVerification } from "@/features/kyc/model/use-start-verification";
import { StartVerificationButton, VerificationOutcome } from "@/features/kyc/ui/verification-controls";
import { Hairline, RowLabel } from "@/shared/ui/list-card";

// Where a user begins identity verification from their profile. It sits inside
// `card-Verification`, below the rows that state what the hub currently holds, so the card
// reads as status-then-action rather than as a form. The money screens offer the same start
// as a block instead of a row — see ./verification-required.

export function StartVerificationRow() {
  const t = useT();
  const start = useStartVerification();

  return (
    <>
      <Hairline />
      <div className="flex flex-col py-3.5">
        <div className="flex min-w-0 items-center justify-between gap-3">
          <RowLabel title={t("profile.kyc.startTitle")} sub={t("profile.kyc.startSub")} />
          {/* i18n-max: 12 — a `shrink-0` Button beside the `min-w-0` row label. */}
          <StartVerificationButton start={start} size="sm" label={t("profile.kyc.start")} className="rounded-lg font-semibold" />
        </div>
        {/* The row has no gap of its own, so the message brings its own margin. */}
        <VerificationOutcome start={start} className="mt-2" />
      </div>
    </>
  );
}
