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

import type { Translate } from "@evinvest/i18n";
import { useLocale, useT } from "@evinvest/i18n/react";
import { Alert, AlertDescription, AlertTitle } from "@evinvest/uikit";

import { type ConsiliumRefusal } from "@/shared/lib/consilium-refusal";
import { formatMoment } from "@/shared/lib/datetime";

/** What the refused consilium was to decide — picks the copy, never the classification. */
export type RefusalSubject = "payout" | "feePolicy";

interface RefusalCopy {
  mailBody: string;
  coolingTitle: string;
  coolingBody: (at: string) => string;
  coolingBodyNoTime: string;
  floorTitle: string;
  floorBody: (n: number) => string;
  floorBodyNoCount: string;
}

// The payout family keeps the bare keys it has always had; a later subject gets its own.
const COPY: Record<RefusalSubject, (t: Translate) => RefusalCopy> = {
  payout: (t) => ({
    mailBody: t("admin.refusal.mailBody", "Owners receive their approval link by email, and no mailer is wired — so a proposal opened now could never be voted on and would simply expire. This has to be configured before the fund can pay itself out."),
    coolingTitle: t("admin.refusal.coolingTitle", "Payouts are paused while the roster settles"),
    coolingBody: (at) => t("admin.refusal.coolingBody", "The list of owners changed within the last 48 hours. Payouts resume at {at}. The pause is deliberate: it keeps a change of ownership and a payout as two separate, visible events rather than one motion.", { at }),
    coolingBodyNoTime: t("admin.refusal.coolingBodyNoTime", "The list of owners changed within the last 48 hours, and payouts stay paused until that period lifts. The pause is deliberate: it keeps a change of ownership and a payout as two separate, visible events rather than one motion."),
    floorTitle: t("admin.refusal.floorTitle", "This fund cannot authorise a payout"),
    floorBody: (n) => t("admin.refusal.floorBody", "A payout needs more than half of the owners to agree, and with {n, plural, one {# owner} other {# owners}} that bar can never be met. Adding owners restores it.", { n }),
    floorBodyNoCount: t("admin.refusal.floorBodyNoCount", "A payout needs more than half of the owners to agree, and this fund has too few owners for that bar to be met. Adding owners restores it."),
  }),
  feePolicy: (t) => ({
    mailBody: t("admin.refusal.feePolicy.mailBody", "Owners receive their approval link by email, and no mailer is wired — so a proposal opened now could never be voted on and would simply expire. This has to be configured before the owners can be asked about a change of terms."),
    coolingTitle: t("admin.refusal.feePolicy.coolingTitle", "Changes of terms are paused while the roster settles"),
    coolingBody: (at) => t("admin.refusal.feePolicy.coolingBody", "The list of owners changed within the last 48 hours. The owners can be asked again at {at}. The pause is deliberate: it keeps a change of ownership and a change of terms as two separate, visible events rather than one motion.", { at }),
    coolingBodyNoTime: t("admin.refusal.feePolicy.coolingBodyNoTime", "The list of owners changed within the last 48 hours, and changes of terms stay paused until that period lifts. The pause is deliberate: it keeps a change of ownership and a change of terms as two separate, visible events rather than one motion."),
    floorTitle: t("admin.refusal.feePolicy.floorTitle", "This fund cannot put a change of terms to its owners"),
    floorBody: (n) => t("admin.refusal.feePolicy.floorBody", "Tightening the terms beyond the house envelope needs more than half of the owners to agree, and with {n, plural, one {# owner} other {# owners}} that bar can never be met. Adding owners restores it; a change within the envelope needs no vote.", { n }),
    floorBodyNoCount: t("admin.refusal.feePolicy.floorBodyNoCount", "Tightening the terms beyond the house envelope needs more than half of the owners to agree, and this fund has too few owners for that bar to be met. Adding owners restores it; a change within the envelope needs no vote."),
  }),
};

export function RefusalNotice({ refusal, liftsAt, subject = "payout" }: { refusal: ConsiliumRefusal; liftsAt: string | null; subject?: RefusalSubject }) {
  const t = useT();
  const locale = useLocale();
  const words = COPY[subject](t);

  const { icon, title, body } =
    refusal.kind === "mail-not-configured"
      ? {
          icon: <MailWarning className="size-4 text-accent-warn" />,
          title: t("admin.refusal.mailTitle", "Governance mail is not configured"),
          body: words.mailBody,
        }
      : refusal.kind === "cooling-off"
        ? {
            icon: <Clock className="size-4 text-accent-warn" />,
            title: words.coolingTitle,
            // Without a parseable deadline the condition is still named — better than a
            // sentence with a hole in it where the time should be.
            body: liftsAt ? words.coolingBody(formatMoment(liftsAt, locale)) : words.coolingBodyNoTime,
          }
        : {
            icon: <ShieldAlert className="size-4 text-accent-warn" />,
            title: words.floorTitle,
            body: refusal.ownerCount === null ? words.floorBodyNoCount : words.floorBody(refusal.ownerCount),
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
