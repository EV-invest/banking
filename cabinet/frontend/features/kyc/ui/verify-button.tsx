"use client";

import { useT } from "@evinvest/i18n/react";

import { useState } from "react";

import { Button, type ButtonSize } from "@evinvest/uikit";

import { useKycStatus } from "@/features/kyc/model/use-kyc-status";
import { VerificationDialog } from "@/features/kyc/ui/verification-dialog";

// The bare trigger: a button that opens the one explanation of verification, for a surface
// outside this slice that has its own frame to put it in — today the first-run checklist on
// Home (#382). Everything the slice decides stays decided here: the dialog it opens, and
// that a running case the reader can re-enter is worded as a continuation (concierge#55)
// rather than as a fresh start.
//
// The caller is trusted to render this only where a start may be offered — the checklist
// knows that from the same `useKycStatus` — so there is no gate of its own to disagree with.

export function VerifyButton({ size, className }: { size?: ButtonSize; className?: string }) {
  const t = useT();
  const { runningCase } = useKycStatus();
  const [open, setOpen] = useState(false);
  return (
    <>
      {/* i18n-max: 14 — see ./verification-required, which declares the same budget. */}
      <Button type="button" size={size} className={className} onClick={() => setOpen(true)}>
        {runningCase !== null ? t("profile.kyc.continue") : t("kyc.verifyNow")}
      </Button>
      <VerificationDialog open={open} onOpenChange={setOpen} />
    </>
  );
}
