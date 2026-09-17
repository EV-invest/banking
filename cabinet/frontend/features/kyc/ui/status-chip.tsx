"use client";

import { useT } from "@evinvest/i18n/react";

import { Skeleton } from "@evinvest/uikit";

import { profileResource } from "@/entities/user/model/profile-resource";
import { kycChipState, type KycChipTone } from "@/features/kyc/lib/chip-state";
import { useKycStatus } from "@/features/kyc/model/use-kyc-status";
import { cn } from "@/shared/lib/cn";
import { useResource } from "@/shared/lib/resource";
import { Pill } from "@/shared/ui/list-card";

// The shell's standing answer to "am I verified?" (#395). Until this, the state was visible
// only on /profile and in a banner Home lets the reader dismiss — after which Deposit,
// Withdraw and Subscribe all led to the gate with no warning. Two presentations of one
// read, and they deliberately differ: the rail chip is a persistent status and shows all
// four states; the mobile tab's dot means "something to do here" and is not drawn once
// verified — a green dot every verified reader carried forever would teach them to ignore
// it. Neither is a link of its own — both sit INSIDE the row or tab that already navigates
// to where the verification row lives, so a nested anchor is never rendered.
//
// Nothing is drawn until the state is known — neither while the reads are in flight nor
// after both have failed. `useKycStatus` answers `level: 0` in both, and a chip that took
// that for tier 0 would paint "Not verified" at every verified reader for a beat on every
// cold load, and for the length of any outage — the flash this whole surface must not have.
// "Settled" is the wallet gate's rule (`../model/use-kyc-gate`): the plane answered, or the
// profile did.

function useKycChip() {
  const { level, runningCase, known, loading } = useKycStatus();
  const { data: profile } = useResource(profileResource);
  return { loading, chip: kycChipState({ level, runningCase, settled: known || profile != null }) };
}

const DOT_TONE: Record<KycChipTone, string> = {
  success: "bg-positive",
  pending: "bg-accent-warn",
  error: "bg-accent-error",
  neutral: "bg-ink-soft",
};

/**
 * The rail chip. `active` is the row's own state: on the filled row every tone would sit on
 * the teal, so there the chip takes the row's ink the way the unread pill does.
 */
export function KycStatusChip({ active = false, className }: { active?: boolean; className?: string }) {
  const t = useT();
  const { loading, chip } = useKycChip();
  // As wide as the widest of the five translations of the longest label, so the row does not
  // shift when the state lands.
  if (loading) return <Skeleton className={cn("h-5 w-24 shrink-0 rounded-full", className)} />;
  if (chip === null) return null;
  return (
    // i18n-max: 15 — a `shrink-0` pill beside the rail row's `min-w-0` label (248px rail).
    <Pill tone={chip.tone} className={cn("shrink-0", active && "bg-background text-ink", className)}>
      <span data-state={chip.state}>{t(chip.labelKey)}</span>
      {/* The only timing wording the owner approved (`kyc.dialog.timeBody`). The pill has
          room for the state alone, so the ETA rides along for assistive tech: the pill is
          not focusable itself — the row it sits in is — so a tooltip had nothing to open on. */}
      {chip.state === "review" && <span className="sr-only">{t("kyc.chip.reviewEta")}</span>}
    </Pill>
  );
}

/** The mobile tab's badge: a dot in the state's colour, the label for assistive tech only. */
export function KycStatusDot({ className }: { className?: string }) {
  const t = useT();
  const { chip } = useKycChip();
  if (chip === null || chip.state === "verified") return null;
  return (
    <span data-state={chip.state} className={cn("size-2 rounded-full", DOT_TONE[chip.tone], className)}>
      <span className="sr-only">{t(chip.labelKey)}</span>
    </span>
  );
}
