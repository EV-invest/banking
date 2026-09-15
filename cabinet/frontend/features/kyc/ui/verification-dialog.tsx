"use client";

import { useT } from "@evinvest/i18n/react";

import { Clock, IdCard, ShieldCheck } from "lucide-react";
import type { LucideIcon } from "lucide-react";

import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@evinvest/uikit";

import { useStartVerification } from "@/features/kyc/model/use-start-verification";
import { StartVerificationButton, VerificationOutcome } from "@/features/kyc/ui/verification-controls";

// The one explanation of verification in the cabinet, and the one place its primary action
// lives. Every entry point — the profile's identity card, the home banner, and the wallet —
// opens THIS, rather than each writing its own account of what the user is about to do.
//
// It is centred (`Dialog`, not `Sheet` or `Drawer`, both of which are edge-anchored) because
// this is a thing the reader stops to read, not a panel they work alongside.
//
// It is controlled rather than carrying its own trigger: the banner's trigger is a whole card,
// the profile's is a row button, and the wallet's is a link inside a rail — one `DialogTrigger`
// could not be all three.

export function VerificationDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const t = useT();
  const start = useStartVerification();

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("kyc.dialog.title")}</DialogTitle>
          {/* The gate as the hub actually applies it (#179): a deposit ADDRESS is what is
              withheld, so this must not say "you cannot receive funds" — someone who was
              verified once already holds an address and is still credited. */}
          <DialogDescription>{t("kyc.dialog.intro")}</DialogDescription>
        </DialogHeader>

        <div className="flex flex-col gap-4">
          <Step icon={IdCard} title={t("kyc.dialog.needTitle")} body={t("kyc.dialog.needBody")} />
          <Step icon={Clock} title={t("kyc.dialog.timeTitle")} body={t("kyc.dialog.timeBody")} />
          <Step icon={ShieldCheck} title={t("kyc.dialog.afterTitle")} body={t("kyc.dialog.afterBody")} />
        </div>

        {/* Above the footer, not inside it: an outcome is about the attempt just made, and a
            reader whose 503 sits beside the button that produced it has less to reconstruct. */}
        <VerificationOutcome start={start} />

        <DialogFooter>
          <StartVerificationButton start={start} label={t("kyc.dialog.action")} className="w-full sm:w-auto" />
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function Step({ icon: Icon, title, body }: { icon: LucideIcon; title: string; body: string }) {
  return (
    <div className="flex gap-3">
      <span className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-secondary text-ink-mid">
        <Icon className="size-4" aria-hidden />
      </span>
      <div className="min-w-0">
        <p className="text-sm font-semibold text-ink">{title}</p>
        <p className="text-sm leading-snug text-ink-soft">{body}</p>
      </div>
    </div>
  );
}
