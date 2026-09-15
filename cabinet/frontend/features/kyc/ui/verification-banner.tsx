"use client";

import { useT } from "@evinvest/i18n/react";

import { ShieldCheck, X } from "lucide-react";
import { useState, useSyncExternalStore } from "react";

import { Button, Card } from "@evinvest/uikit";

import { bannerDismissedServerSnapshot, bannerDismissedSnapshot, dismissBanner, subscribeBannerDismissal } from "@/features/kyc/lib/banner-dismissal";
import { useKycStatus } from "@/features/kyc/model/use-kyc-status";
import { VerificationDialog } from "@/features/kyc/ui/verification-dialog";
import { cn } from "@/shared/lib/cn";

// The first thing a new account is told about verification. A person who signs up today is
// never told it exists, why it matters, or how little it takes — and the profile card that
// says so is two navigations away (#213).
//
// It lives in the KYC slice rather than in `views/dashboard` because "may this user be offered
// verification" is the slice's knowledge; the view decides only where on the page it sits.
//
// Three conditions, all of which have to hold:
//
//   · tier 0 — above it there is nothing to offer;
//   · no case running — someone who has already submitted is not an un-onboarded newcomer,
//     and the profile is where their case's state is reported;
//   · not dismissed in this browser in the last seven days — see ../lib/banner-dismissal.

export function VerificationBanner({ className }: { className?: string }) {
  const t = useT();
  const { level, runningCase, loading } = useKycStatus();
  const [open, setOpen] = useState(false);
  const dismissed = useSyncExternalStore(subscribeBannerDismissal, bannerDismissedSnapshot, bannerDismissedServerSnapshot);

  if (loading || dismissed || level !== 0 || runningCase !== null) return null;

  return (
    <>
      <Card className={cn("flex flex-row items-start gap-3.5 border-accent-debug/40 p-4 lg:gap-4 lg:p-5", className)}>
        <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-secondary text-accent-debug">
          <ShieldCheck className="size-4.5" aria-hidden />
        </span>
        <div className="flex min-w-0 flex-1 flex-col gap-3 lg:flex-row lg:items-center lg:justify-between lg:gap-5">
          <div className="min-w-0">
            <p className="text-sm font-semibold text-ink">{t("kyc.banner.title")}</p>
            <p className="text-sm leading-snug text-ink-soft">{t("kyc.banner.body")}</p>
          </div>
          {/* i18n-max: 14 — a `shrink-0` button beside a `min-w-0` block from `lg` up. The
              budget is the tightest frame this key appears in, and ./verification-required
              declares the same number for it: two numbers on one string is how a budget
              stops meaning anything. Nothing checks these (scripts/i18n-check.mts compares
              catalogues, never code), so they are worth only as much as they are true —
              de/fr/ru were at 20-21 when this was written and have been cut to fit. */}
          <Button type="button" size="sm" className="shrink-0 self-start font-semibold lg:self-auto" onClick={() => setOpen(true)}>
            {t("kyc.verifyNow")}
          </Button>
        </div>
        {/* uikit's own icon button: the square, the hover surface, the disabled state and a
            focus ring are all things it brings, and the hand-written one had rebuilt half of
            them. */}
        <Button
          type="button"
          variant="ghost"
          size="sm"
          icon
          onClick={() => dismissBanner()}
          aria-label={t("kyc.banner.dismiss")}
          className="-mt-1 -mr-1 shrink-0 text-ink-soft hover:text-ink"
        >
          <X aria-hidden />
        </Button>
      </Card>
      <VerificationDialog open={open} onOpenChange={setOpen} />
    </>
  );
}
