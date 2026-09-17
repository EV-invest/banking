"use client";

import { useT } from "@evinvest/i18n/react";

import { Badge, Skeleton } from "@evinvest/uikit";

import { kycChipState, type KycChipTone } from "@/features/kyc/lib/chip-state";
import { useKycStatus } from "@/features/kyc/model/use-kyc-status";
import { cn } from "@/shared/lib/cn";

// The shell's standing answer to "am I verified?" (#395). Until this, the state was visible
// only on /profile and in a banner Home lets the reader dismiss — after which Deposit,
// Withdraw and Subscribe all led to the gate with no warning. Two presentations of one
// read: a chip in the rail's Profile row, and a dot on the mobile tab that leads there.
// Neither is a link of its own — both sit INSIDE the row or tab that already navigates to
// where the verification row lives, so a nested anchor is never rendered.
//
// Nothing is drawn until the state is known. `useKycStatus` starts as `loading` on a cold
// tab, and a chip that guessed would paint "Not verified" at every verified reader for a
// beat on every load — the flash this whole surface must not have.

const CHIP_TONE: Record<KycChipTone, string> = {
  positive: "border-positive/40 text-positive",
  neutral: "border-ink/25 text-ink-mid",
  warn: "border-accent-warn/40 text-accent-warn",
  muted: "border-border text-ink-soft",
};

const DOT_TONE: Record<KycChipTone, string> = {
  positive: "bg-positive",
  neutral: "bg-ink-mid",
  warn: "bg-accent-warn",
  muted: "bg-ink-soft",
};

/**
 * The rail chip. `active` is the row's own state: on the filled row every tone would sit on
 * the teal, so there the chip takes the row's ink the way the unread pill does.
 */
export function KycStatusChip({ active = false, className }: { active?: boolean; className?: string }) {
  const t = useT();
  const { level, runningCase, loading } = useKycStatus();
  if (loading) return <Skeleton className={cn("h-5 w-16 shrink-0 rounded-full", className)} />;
  const chip = kycChipState({ level, runningCase });
  return (
    // i18n-max: 15 — a `shrink-0` badge beside the rail row's `min-w-0` label (248px rail).
    <Badge
      variant="outline"
      data-state={chip.state}
      // The only timing wording the owner approved (`kyc.dialog.timeBody`); the chip has
      // room for the state alone, so the ETA rides on the tooltip.
      title={chip.state === "review" ? t("kyc.chip.reviewEta") : undefined}
      className={cn("shrink-0 rounded-full font-semibold", active ? "border-on-primary/40 text-on-primary" : CHIP_TONE[chip.tone], className)}
    >
      {t(chip.labelKey)}
    </Badge>
  );
}

/**
 * The mobile tab's badge: a dot in the state's colour, the label for assistive tech only.
 * Not drawn once verified — a badge on a tab means something is left to do there, and a
 * green dot every verified reader carries forever would teach them to ignore it.
 */
export function KycStatusDot({ className }: { className?: string }) {
  const t = useT();
  const { level, runningCase, loading } = useKycStatus();
  if (loading) return null;
  const chip = kycChipState({ level, runningCase });
  if (chip.state === "verified") return null;
  return (
    <span data-state={chip.state} className={cn("size-2 rounded-full", DOT_TONE[chip.tone], className)}>
      <span className="sr-only">{t(chip.labelKey)}</span>
    </span>
  );
}
