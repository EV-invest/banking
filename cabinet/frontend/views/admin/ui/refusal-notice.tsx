"use client";

// A refusal the money plane raised on purpose when asked to open a consilium, in words
// that say what to do about it.
//
// Two of the three arrive with the same status and are told apart by their message
// (`shared/lib/consilium-refusal.ts`); all three are conditions rather than faults, so none
// is styled as an error. The cooling-off one shows a clock time rather than the duration
// the backend sent, because a duration is stale the moment it is rendered and an operator
// planning around it has to do arithmetic on it.
//
// Lived on the revenue screen while that was the only place a consilium was opened from;
// it is the payments screen's now, and stays in `views/admin/ui` beside the shell so a
// third opener would not have to reach into either. The fees screen is that third opener
// (banking#323): the conditions are the same three, but a sentence about "payouts" over a
// refused change of terms would tell the operator the wrong thing is paused, so the copy
// is chosen by what was being opened.

import { Clock, MailWarning, ShieldAlert } from "lucide-react";

import { useLocale, useT } from "@evinvest/i18n/react";
import { Alert, AlertDescription, AlertTitle } from "@evinvest/uikit";

import { type ConsiliumRefusal } from "@/shared/lib/consilium-refusal";
import { formatMoment } from "@/shared/lib/datetime";

/** What the refused consilium was to decide — picks the copy, never the classification. */
export type RefusalSubject = "payout" | "feePolicy";

export function RefusalNotice({ refusal, liftsAt, subject = "payout" }: { refusal: ConsiliumRefusal; liftsAt: string | null; subject?: RefusalSubject }) {
  const t = useT();
  const locale = useLocale();
  // The payout family keeps the bare keys it has always had; a later subject gets its own.
  const words = subject === "payout" ? "admin.refusal" : "admin.refusal.feePolicy";

  const { icon, title, body } =
    refusal.kind === "mail-not-configured"
      ? {
          icon: <MailWarning className="size-4 text-accent-warn" />,
          title: t("admin.refusal.mailTitle"),
          body: t(`${words}.mailBody`),
        }
      : refusal.kind === "cooling-off"
        ? {
            icon: <Clock className="size-4 text-accent-warn" />,
            title: t(`${words}.coolingTitle`),
            // Without a parseable deadline the condition is still named — better than a
            // sentence with a hole in it where the time should be.
            body: liftsAt ? t(`${words}.coolingBody`, { at: formatMoment(liftsAt, locale) }) : t(`${words}.coolingBodyNoTime`),
          }
        : {
            icon: <ShieldAlert className="size-4 text-accent-warn" />,
            title: t(`${words}.floorTitle`),
            body: refusal.ownerCount === null ? t(`${words}.floorBodyNoCount`) : t(`${words}.floorBody`, { n: refusal.ownerCount }),
          };

  // The house callout: `Alert` with the amber tint, as `BreakGlassNotice` draws it.
  return (
    <Alert role="status" className="border-accent-warn/40 bg-accent-warn/10">
      {icon}
      <AlertTitle>{title}</AlertTitle>
      <AlertDescription className="text-ink">{body}</AlertDescription>
    </Alert>
  );
}
