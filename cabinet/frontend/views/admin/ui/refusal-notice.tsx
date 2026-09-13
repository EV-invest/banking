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
// third opener would not have to reach into either.

import { Clock, MailWarning, ShieldAlert } from "lucide-react";

import { useLocale, useT } from "@evinvest/i18n/react";
import { Alert, AlertDescription, AlertTitle } from "@evinvest/uikit";

import { type ConsiliumRefusal } from "@/shared/lib/consilium-refusal";
import { formatMoment } from "@/shared/lib/datetime";

export function RefusalNotice({ refusal, liftsAt }: { refusal: ConsiliumRefusal; liftsAt: string | null }) {
  const t = useT();
  const locale = useLocale();

  const { icon, title, body } =
    refusal.kind === "mail-not-configured"
      ? {
          icon: <MailWarning className="size-4 text-main-accent-t3" />,
          title: t("admin.refusal.mailTitle"),
          body: t("admin.refusal.mailBody"),
        }
      : refusal.kind === "cooling-off"
        ? {
            icon: <Clock className="size-4 text-main-accent-t3" />,
            title: t("admin.refusal.coolingTitle"),
            // Without a parseable deadline the condition is still named — better than a
            // sentence with a hole in it where the time should be.
            body: liftsAt ? t("admin.refusal.coolingBody", { at: formatMoment(liftsAt, locale) }) : t("admin.refusal.coolingBodyNoTime"),
          }
        : {
            icon: <ShieldAlert className="size-4 text-main-accent-t3" />,
            title: t("admin.refusal.floorTitle"),
            body: refusal.ownerCount === null ? t("admin.refusal.floorBodyNoCount") : t("admin.refusal.floorBody", { n: refusal.ownerCount }),
          };

  // The house callout: `Alert` with the amber tint, as `BreakGlassNotice` draws it.
  return (
    <Alert role="status" className="border-main-accent-t3/40 bg-main-accent-t3/10">
      {icon}
      <AlertTitle>{title}</AlertTitle>
      <AlertDescription className="text-foreground">{body}</AlertDescription>
    </Alert>
  );
}
