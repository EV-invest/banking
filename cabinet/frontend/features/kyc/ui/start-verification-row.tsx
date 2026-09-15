"use client";

import { useT } from "@evinvest/i18n/react";

import { useState } from "react";

import { Button } from "@evinvest/uikit";

import { useKycStatus } from "@/features/kyc/model/use-kyc-status";
import { VerificationDialog } from "@/features/kyc/ui/verification-dialog";
import { Hairline, RowLabel } from "@/shared/ui/list-card";

// Where a user begins — or picks up — identity verification from their profile. It sits inside
// `card-Verification`, below the rows that state what the hub currently holds, so the card
// reads as status-then-action rather than as a form.
//
// Three states, because the tier alone was never enough to tell them apart (#190):
//
//   · nothing running — the invitation;
//   · a running case the user can re-enter — the same action, worded as a continuation. A
//     repeat start returns the SAME vendor session (concierge#55), so this costs nothing;
//   · a running case with nowhere to return to — a sentence and no button. The case holds the
//     start gate, so the button could only fail, and the user is waiting on a decision.
//
// The button opens the dialog rather than starting: what the user is about to hand over, and
// how long it takes, is said once and in one place (#213) — this row is the entry, not the
// explanation.

export function StartVerificationRow() {
  const t = useT();
  const { level, runningCase, canStart } = useKycStatus();
  const [open, setOpen] = useState(false);

  // The plane can know the tier has moved before the profile's mirror of it does; when it
  // does, this row is already history and must not offer to spend another vendor session.
  if (level > 0) return null;

  const running = runningCase !== null;
  return (
    <>
      <Hairline />
      <div className="flex min-w-0 items-center justify-between gap-3 py-3.5">
        <RowLabel
          title={running ? t("profile.kyc.caseTitle") : t("profile.kyc.startTitle")}
          sub={running ? (canStart ? t("profile.kyc.caseResumeSub") : t("profile.kyc.caseReviewSub")) : t("profile.kyc.startSub")}
        />
        {/* i18n-max: 12 — a `shrink-0` Button beside the `min-w-0` row label. */}
        {canStart && (
          <Button type="button" size="sm" className="shrink-0 rounded-lg font-semibold" onClick={() => setOpen(true)}>
            {running ? t("profile.kyc.continue") : t("profile.kyc.start")}
          </Button>
        )}
      </div>
      <VerificationDialog open={open} onOpenChange={setOpen} />
    </>
  );
}
