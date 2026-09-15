"use client";

import { useT } from "@evinvest/i18n/react";

import { useKycStatus } from "@/features/kyc/model/use-kyc-status";
import { useStartVerification } from "@/features/kyc/model/use-start-verification";
import { StartVerificationButton, VerificationOutcome } from "@/features/kyc/ui/verification-controls";
import { Hairline, RowLabel } from "@/shared/ui/list-card";

// Where a user begins — or picks up — identity verification from their profile. It sits inside
// `card-Verification`, below the rows that state what the hub currently holds, so the card
// reads as status-then-action rather than as a form. The money screens offer the same start
// as a block instead of a row — see ./verification-required.
//
// Three states, because the tier alone was never enough to tell them apart (#190):
//
//   · nothing running — the invitation, as before;
//   · a running case the user can re-enter — the same action, worded as a continuation. A
//     repeat start returns the SAME vendor session (concierge#55), so this costs nothing;
//   · a running case with nowhere to return to — a sentence and no button. The case holds the
//     start gate, so the button could only fail, and the user is waiting on a decision.

export function StartVerificationRow() {
  const t = useT();
  const { level, runningCase, canStart } = useKycStatus();
  const start = useStartVerification();

  // The plane can know the tier has moved before the profile's mirror of it does; when it
  // does, this row is already history and must not offer to spend another vendor session.
  if (level > 0) return null;

  const running = runningCase !== null;
  return (
    <>
      <Hairline />
      <div className="flex flex-col py-3.5">
        <div className="flex min-w-0 items-center justify-between gap-3">
          <RowLabel
            title={running ? t("profile.kyc.caseTitle") : t("profile.kyc.startTitle")}
            sub={running ? (canStart ? t("profile.kyc.caseResumeSub") : t("profile.kyc.caseReviewSub")) : t("profile.kyc.startSub")}
          />
          {/* i18n-max: 12 — a `shrink-0` Button beside the `min-w-0` row label. */}
          {canStart && (
            <StartVerificationButton
              start={start}
              size="sm"
              label={running ? t("profile.kyc.continue") : t("profile.kyc.start")}
              className="rounded-lg font-semibold"
            />
          )}
        </div>
        {/* The row has no gap of its own, so the message brings its own margin. */}
        <VerificationOutcome start={start} className="mt-2" />
      </div>
    </>
  );
}
