"use client";

import { useT } from "@evinvest/i18n/react";

import { ShieldCheck } from "lucide-react";
import { useState } from "react";

import { Button, Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@evinvest/uikit";

import { VerificationDialog } from "@/features/kyc/ui/verification-dialog";
import { cn } from "@/shared/lib/cn";

/**
 * What stands where a money surface would be for a caller the hub has not cleared yet.
 *
 * Not a failure box. The hub refuses a deposit address and a withdrawal below tier 1 on
 * purpose, and it says so distinctly — but the deposit screen flattened that into "deposit
 * address unavailable" over "this rail is still being provisioned", on every rail at once.
 * That reads as an outage the reader can only wait out, for a state they can leave in one
 * click. Withdraw had the mirror of it: a form that filled in completely and refused on
 * submit.
 *
 * Title and description are the caller's because what is closed differs between the screens;
 * the way out is identical, so it lives here — and it is the SAME dialog the profile card and
 * the home banner open (#213/#215), so the account of what verification involves is written
 * once rather than three times.
 */
export function VerificationRequired({ title, description, className }: { title: string; description: string; className?: string }) {
  const t = useT();
  const [open, setOpen] = useState(false);

  return (
    // uikit's Empty draws a dashed frame but leaves the border width to the caller, and
    // doubles its padding at `md` — the call the dashboard and consilium already make.
    <Empty className={cn("border md:p-6", className)}>
      <EmptyHeader>
        <EmptyMedia variant="icon">
          <ShieldCheck />
        </EmptyMedia>
        <EmptyTitle>{title}</EmptyTitle>
        <EmptyDescription>{description}</EmptyDescription>
      </EmptyHeader>
      <EmptyContent>
        {/* i18n-max: 16 — the uikit Button is shrink-0. */}
        <Button type="button" onClick={() => setOpen(true)}>
          {t("kyc.verifyNow")}
        </Button>
      </EmptyContent>
      <VerificationDialog open={open} onOpenChange={setOpen} />
    </Empty>
  );
}
